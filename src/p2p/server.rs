use tokio_tungstenite::{
	tungstenite::Message, WebSocketStream, accept_async
};
use tokio::net::{
	TcpListener, TcpStream
};
use std::{
	thread, collections::HashMap, net::{ SocketAddr }, time::{
		Duration, SystemTime
	}, sync::{
		RwLock, mpsc::{
			Sender, Receiver, channel, sync_channel, SyncSender
		}
	}
};
use serde::{
	Serialize, de::DeserializeOwned
};
use serde_json::{
	Value, from_str, from_value, to_string, json
};
use anyhow::{
	Result, anyhow
};
use super::{
	Wrapper, Payload, Error, Caller
};
use futures::{
	future::BoxFuture, executor::block_on, stream::SplitSink
};
use futures_util::{
	SinkExt, StreamExt
};

lazy_static! {
	static ref STOP: RwLock<bool> = RwLock::new(false);
	static ref CLIENTS: RwLock<HashMap<i32, Option<SplitSink<WebSocketStream<TcpStream>, Message>>>> = RwLock::new(HashMap::new());
	static ref HEARTBEATS: RwLock<HashMap<i32, SystemTime>> = RwLock::new(HashMap::new());
	static ref SERVER_CLIENTS: RwLock<HashMap<i32, SyncSender<String>>> = RwLock::new(HashMap::new());
	static ref SERVER_REGISTRY: RwLock<HashMap<String, Box<dyn Fn(i32, Value) -> BoxFuture<'static, Result<Value, String>> + Send + Sync + 'static>>> = RwLock::new(HashMap::new());
	static ref CALLBACK: RwLock<Option<Box<dyn Fn(i32, Option<HashMap<String, Receiver<String>>>) + Send + Sync + 'static>>> = RwLock::new(None);
}

// a server instance for handling registering both request/response methods and listening at none-blocking mode
pub struct Server {
	client_registry: Vec<String>,
	socket:          SocketAddr
}

fn callback(client_id: i32, receivers: Option<HashMap<String, Receiver<String>>>) {
	if let Some(cb) = &*CALLBACK.read().unwrap() {
		cb(client_id, receivers);
	}
}

fn close_client(id: i32) {
	let prev = CLIENTS.write().unwrap().insert(id, None);
	if prev.is_some() && prev.unwrap().is_some() {
		println!("#{} callback closed", id);
		callback(id, None);
	}
}

fn udapte_heartbeat(id: i32) {
	*HEARTBEATS.write().unwrap().entry(id).or_insert(SystemTime::now()) = SystemTime::now();
}

fn client_send(id: i32, msg: Message) {
	let mut ok = true;
	if let Some(client) = CLIENTS.write().unwrap().get_mut(&id).unwrap() {
		if let Message::Close(_) = msg {
			ok = false;
		}
		if let Err(err) = block_on(client.send(msg)) {
			println!("#{} send message error: {}", id, err.to_string());
			ok = false;
		}
	} else {
		panic!("send on #{} CLOSED client", id);
	}
	if !ok {
		close_client(id);
	}
}

impl Server {
	pub fn new(socket: &str) -> Self {
		Server {
			client_registry: Vec::new(),
			socket:          socket.parse().expect("parse socket string")
		}
	}

	// register a function instance to respond client request
	pub fn register<F>(self, name: &str, method: F) -> Self
		where
			F: Fn(i32, Value) -> BoxFuture<'static, Result<Value, String>> + Send + Sync + 'static
	{
		SERVER_REGISTRY.write().unwrap().insert(String::from(name), Box::new(method));
		self
	}

	// register a function name that enables server to send request to client
	pub fn register_call(mut self, name: &str) -> Self {
		self.client_registry.push(String::from(name));
		self
	}

	// listen connections at non-blocking mode
	pub async fn listen<F>(self, sleep_ms: u64, max_connection: u8, local_callback: F) -> Result<ServerClient> 
		where
			F: Fn(i32, Option<HashMap<String, Receiver<String>>>) + Send + Sync + 'static
	{
		let server = TcpListener::bind(self.socket).await?;
		let (writer, reader) = channel::<(i32, String)>();
		*STOP.write().unwrap() = false;
		*CALLBACK.write().unwrap() = Some(Box::new(local_callback));
		// start p2p server controller thread
		tokio::spawn(async move {
			let mut sleep = tokio::time::interval(Duration::from_millis(sleep_ms));
			while !*STOP.read().unwrap() {
				// receiving message from server controller
				if let Ok((client_id, message)) = reader.try_recv() {
					if client_id > 0 {
						// send to specified serverclient
						match SERVER_CLIENTS.write().unwrap().get(&client_id) {
							Some(serverclient) => serverclient.send(message).unwrap(),
							None => println!("sending message {} to client #{} failed", message, client_id)
						}
					} else {
						if message == String::from("_SHUTDOWN_") {
							*STOP.write().unwrap() = true;
						}
						// send to all serverclients
						for (client_id, _) in &*CLIENTS.write().unwrap() {
							SERVER_CLIENTS.write().unwrap().get(client_id).unwrap().send(message.clone()).unwrap();
						}
					}
				}
				sleep.tick().await;
				println!("tick");
			}
			println!("p2p server CONTROLLER thread closed");
		});
		// start p2p server worker thread
		tokio::spawn(async move {
			let mut client_id = 0;
			while !*STOP.read().unwrap() {
				// listening client connection
				let (connection, _) = server.accept().await.expect("accept connection");
				let client = accept_async(connection).await;
				if let Err(err) = client {
					println!("accept error => {}", err);
					continue
				}
				if max_connection > 0 && CLIENTS.write().unwrap().len() >= max_connection as usize {
					client.unwrap().send(Message::Close(None)).await.unwrap();
					continue
				}
				client_id += 1;
				let (client_writer, client_reader) = sync_channel(4096);
				SERVER_CLIENTS.write().unwrap().insert(client_id, client_writer);
				let mut client_sender = HashMap::new();
				let mut client_receiver = HashMap::new();
				for name in &self.client_registry {
					let (cs, cr) = channel();
					client_sender.insert(name.clone(), cs);
					client_receiver.insert(name.clone(), cr);
				}
				callback(client_id, Some(client_receiver));
				let (sink, mut stream) = client.unwrap().split();
				CLIENTS.write().unwrap().insert(client_id, Some(sink));
				udapte_heartbeat(client_id);
				let this_client_id = client_id;
				// start read thread for current connection
				tokio::spawn(async move { loop {
					if CLIENTS.read().unwrap().get(&this_client_id).unwrap().is_none() {
						println!("p2p serverclient #{} READ thread closed", this_client_id);
						return
					}
					// receiving calling messages from client
					let next = stream.next().await;
					if let None = next {
						continue
					}
					match next.unwrap() {
						Ok(Message::Text(value)) => {
							let message: Wrapper = {
								let value = from_str(value.as_str()).expect("parse client message");
								from_value(value).unwrap()
							};
							match message {
								Wrapper::Send(payload) => {
									// searching in server response registry table
									if let Some(function) = SERVER_REGISTRY.read().unwrap().get(&payload.name) {
										let params = from_str(payload.body.as_str()).unwrap();
										let (send, receive) = channel();
										let future = function(this_client_id, params);
										// wait data process
										thread::spawn(move || {
											let body;
											let response = {
												match block_on(async move { future.await }) {
													Ok(result)  => body = to_string(&result).unwrap(),
													Err(reason) => body = to_string(&json!(Error { reason })).unwrap()
												}
												to_string(
													&Wrapper::Reply(Payload { name: payload.name, body })
												).unwrap()
											};
											send.send(response).unwrap();
										});
										// wait to emit processed data
										thread::spawn(move || {
											if let Ok(response) = receive.recv() {
												client_send(this_client_id, Message::text(response));
											}
										});
									} else {
										panic!("method {} can't find in server registry table", payload.name);
									}
								},
								Wrapper::Reply(payload) => {
									// searching in client message registry sender table
									if let Some(sender) = client_sender.get(&payload.name) {
										sender.send(payload.body).unwrap();
									} else {
										panic!("method {} can't find in client registry table", payload.name);
									}
								}
							}
						},
						Ok(Message::Close(_)) => close_client(this_client_id),
						Ok(Message::Ping(_)) => {
							udapte_heartbeat(this_client_id);
							client_send(this_client_id, Message::Pong(vec![]));
						},
						Err(err) => {
							println!("serverclient #{} next => {}", this_client_id, err);
							close_client(this_client_id);
						},
						_ => panic!("unsupported none-text type message from client")
					}
				}});
				// start write thread for current connection
				tokio::spawn(async move {
					let mut sleep = tokio::time::interval(Duration::from_millis(sleep_ms));
					loop {
						if CLIENTS.read().unwrap().get(&this_client_id).unwrap().is_none() {
							println!("p2p serverclient #{} WRITE thread closed", this_client_id);
							return
						}
						// fetching message from server client
						if let Ok(msg) = client_reader.try_recv() {
							if msg == String::from("_SHUTDOWN_") {
								client_send(this_client_id, Message::Close(None));
							} else {
								client_send(this_client_id, Message::text(msg));
							}
						}
						// check connection alive status
						let last_ping = *HEARTBEATS.read().unwrap().get(&this_client_id).unwrap();
						if SystemTime::now().duration_since(last_ping).unwrap() > Duration::from_secs(8) {
							close_client(this_client_id);
						}
						sleep.tick().await;
					}
				});
			}
			println!("p2p server WORKER thread closed");
		});
		Ok(ServerClient::new(writer))
	}
}

// serverclient representing one connecting which generated after the server accepted one client
// to handle request from server to that client
pub struct ServerClient {
	writer:           Sender<(i32, String)>,
	client_receivers: HashMap<i32, HashMap<String, Receiver<String>>>,
	client_id:        i32
}

impl ServerClient {
	pub fn new(writer: Sender<(i32, String)>) -> Self {
		ServerClient {
			writer:           writer,
			client_receivers: HashMap::new(),
			client_id:        0
		}
	}

	pub fn active(&self) -> bool {
		if let Ok(clients) = CLIENTS.read() {
			!clients.is_empty()
		} else {
			false
		}
	}

	pub fn shutdown(&self) {
		self.writer.send((0, String::from("_SHUTDOWN_"))).unwrap();
	}

	pub fn append_receivers(&mut self, client_id: i32, client_receivers: HashMap<String, Receiver<String>>) {
		self.client_receivers.insert(client_id, client_receivers);
	}

	pub fn set_id(&mut self, client_id: i32) -> &mut Self {
		self.client_id = client_id;
		self
	}
}

impl Caller for ServerClient {
	fn call<T: Serialize, R: DeserializeOwned>(&self, name: &str, params: T) -> Result<R> {
		if !self.active() {
			return Err(anyhow!("no client connected"));
		}
		if self.client_id == 0 {
			return Err(anyhow!("empty client_id"));
		}
		if let Some(receivers) = self.client_receivers.get(&self.client_id) {
			if let Some(receiver) = receivers.get(&String::from(name)) {
				let request = to_string(
					&Wrapper::Send(
						Payload {
							name: String::from(name),
							body: to_string(&json!(params))?
						}
					)
				)?;
				self.writer.send((self.client_id, request))?;
				let value: R = {
					let response: Value = from_str(receiver.recv()?.as_str())?;
					if let Ok(error) = from_value::<Error>(response.clone()) {
						return Err(anyhow!("error from server: {}", error.reason));
					}
					from_value(response)?
				};
				Ok(value)
			} else {
				Err(anyhow!("method {} isn't registered", name))
			}
		} else {
			Err(anyhow!("no client id #{}", self.client_id))
		}
	}
}
