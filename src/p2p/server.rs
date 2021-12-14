use websocket::{
	message::OwnedMessage, sender::Writer, sync::{
		Server as WsServer
	}
};
use std::{
	thread, net::{
		SocketAddr, TcpStream
	}, collections::HashMap, time::{
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
	future::BoxFuture, executor::block_on
};

lazy_static! {
	static ref STOP: RwLock<bool> = RwLock::new(false);
	static ref CLIENTS: RwLock<HashMap<i32, Option<Writer<TcpStream>>>> = RwLock::new(HashMap::new());
	static ref HEARTBEATS: RwLock<HashMap<i32, SystemTime>> = RwLock::new(HashMap::new());
	static ref SERVER_CLIENTS: RwLock<HashMap<i32, SyncSender<String>>> = RwLock::new(HashMap::new());
	static ref SERVER_REGISTRY: RwLock<HashMap<String, Box<dyn Fn(i32, Value) -> BoxFuture<'static, Result<Value, String>> + Send + Sync + 'static>>> = RwLock::new(HashMap::new());
	static ref CALLBACK: RwLock<Option<Box<dyn Fn(i32, Option<HashMap<String, Receiver<String>>>) + Send + Sync + 'static>>> = RwLock::new(None);
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

fn client_send(id: i32, msg: OwnedMessage) {
	let mut ok = true;
	if let Some(client) = CLIENTS.write().unwrap().get_mut(&id).unwrap() {
		if let OwnedMessage::Close(_) = msg {
			ok = false;
		}
		if let Err(err) = client.send_message(&msg) {
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

// a server instance for handling registering both request/response methods and listening at none-blocking mode
pub struct Server {
	client_registry: Vec<String>,
	socket:          SocketAddr
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

	// listen connections at none-blocking mode
	pub fn listen<F>(self, sleep_ms: u64, max_connection: u8, local_callback: F) -> Result<ServerClient> 
		where
			F: Fn(i32, Option<HashMap<String, Receiver<String>>>) + Send + Sync + 'static
	{
		let mut server = WsServer::bind(self.socket)?;
		*STOP.write().unwrap() = false;
		*CALLBACK.write().unwrap() = Some(Box::new(local_callback));
		let (writer, reader) = channel::<(i32, String)>();
		// start p2p server controller thread
		thread::spawn(move || {
			let sleep_ms = sleep_ms.clone();
			loop {
				if *STOP.read().unwrap() {
					println!("p2p server CONTROLLER thread closed");
					return
				}
				// receiving message from server controller
				if let Ok((client_id, message)) = reader.try_recv() {
					assert!(message.len() < 4096);
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
				thread::sleep(Duration::from_millis(sleep_ms));
			}
		});
		// start p2p server worker thread
		thread::spawn(move || {
			let mut client_id = 0;
			loop {
				if *STOP.read().unwrap() {
					println!("p2p server WORKER thread closed");
					return
				}
				// listening client connection
				let connection = server.accept().unwrap();
				if max_connection > 0 && CLIENTS.write().unwrap().len() >= max_connection as usize {
					connection.reject().unwrap();
					continue
				}
				let client = connection.accept().unwrap();
				client_id += 1;
				let (client_writer, client_reader) = sync_channel(4096);
				SERVER_CLIENTS.write().unwrap().insert(client_id, client_writer);
				let mut response_sender = HashMap::new();
				let mut response_receiver = HashMap::new();
				for name in &self.client_registry {
					let (cs, cr) = channel();
					response_sender.insert(name.clone(), cs);
					response_receiver.insert(name.clone(), cr);
				}
				callback(client_id, Some(response_receiver));
				let (mut stream, sink) = client.split().unwrap();
				CLIENTS.write().unwrap().insert(client_id, Some(sink));
				udapte_heartbeat(client_id);
				let this_client_id = client_id;
				// start read thread for current connection
				thread::spawn(move || loop {
					if CLIENTS.read().unwrap().get(&this_client_id).unwrap().is_none() {
						println!("p2p serverclient #{} READ thread closed", this_client_id);
						return
					}
					// receiving calling messages from client
					let recv = stream.recv_message();
					if let Err(err) = recv {
						println!("serverclient #{} next => {}", this_client_id, err);
						close_client(this_client_id);
						continue
					}
					match recv.unwrap() {
						OwnedMessage::Text(value) => {
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
												client_send(this_client_id, OwnedMessage::Text(response));
											}
										});
									} else {
										panic!("method {} can't find in server registry table", payload.name);
									}
								},
								Wrapper::Reply(payload) => {
									// searching in client message registry sender table
									if let Some(sender) = response_sender.get(&payload.name) {
										sender.send(payload.body).unwrap();
									} else {
										panic!("method {} can't find in client registry table", payload.name);
									}
								}
							}
						},
						OwnedMessage::Close(_) => close_client(this_client_id),
						OwnedMessage::Ping(_) => {
							udapte_heartbeat(this_client_id);
							client_send(this_client_id, OwnedMessage::Pong(vec![]));
						},
						_ => panic!("unsupported none-text type message from client")
					}
				});
				// start write thread for current connection
				thread::spawn(move || {
					let sleep_ms = sleep_ms.clone();
					loop {
						if CLIENTS.read().unwrap().get(&this_client_id).unwrap().is_none() {
							println!("p2p serverclient #{} WRITE thread closed", this_client_id);
							return
						}
						// fetching message from server client
						if let Ok(msg) = client_reader.try_recv() {
							if msg == String::from("_SHUTDOWN_") {
								client_send(this_client_id, OwnedMessage::Close(None));
							} else {
								client_send(this_client_id, OwnedMessage::Text(msg));
							}
						}
						// check connection alive status
						let last_ping = *HEARTBEATS.read().unwrap().get(&this_client_id).unwrap();
						if SystemTime::now().duration_since(last_ping).unwrap() > Duration::from_secs(8) {
							close_client(this_client_id);
						}
						thread::sleep(Duration::from_millis(sleep_ms));
					}
				});
			}
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
		self.writer
			.send((0, String::from("_SHUTDOWN_")))
			.expect("send shutdown");
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
