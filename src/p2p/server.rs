use websocket::{
	Message, sync::Server as WsServer, message::OwnedMessage, result::WebSocketError
};
use std::{
	thread, net::SocketAddr, collections::{
		HashMap, HashSet
	}, time::{
		Duration, SystemTime
	}, sync::{
		RwLock, mpsc::{
			Sender, Receiver, channel
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
	static ref CLIENT: RwLock<HashSet<i32>> = RwLock::new(HashSet::new());
	static ref KICKED_CLIENTS: RwLock<Vec<i32>> = RwLock::new(vec![]);
	static ref SERVER_REGISTRY: RwLock<HashMap<String, 
		Box<dyn Fn(Value) -> BoxFuture<'static, Result<Value, String>> + Send + Sync + 'static>>> = RwLock::new(HashMap::new());
}

// a server instance for handling registering both request/response methods and listening at none-blocking mode,
// but only supports one connection at the same time
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
			F: Fn(Value) -> BoxFuture<'static, Result<Value, String>> + Send + Sync + 'static
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
	pub fn listen<F>(self, sleep_ms: u64, max_connection: u8, callback: F) -> Result<ServerClient> 
		where
			F: Fn(i32, Option<HashMap<String, Receiver<String>>>) + Send + Sync + 'static
	{
		let mut server = WsServer::bind(self.socket)?;
		server.set_nonblocking(true)?;
		let (writer, reader) = channel();
		thread::spawn(move || {
			let mut client_id = 0;
			let mut serverclients: HashMap<i32, Sender<String>> = HashMap::new();
			let mut skip = false;
			loop {
				// listening client connection
				if let Ok(connect) = server.accept() {
					if max_connection > 0 && CLIENT.write().unwrap().len() >= max_connection as usize {
						connect.reject().expect("reject connection");
						continue
					}
					client_id += 1;
					let client = connect.accept().expect("accept connection");
					client.set_nonblocking(true).expect("set blocking");
					let (client_writer, client_reader) = channel();
					serverclients.insert(client_id, client_writer);
					let mut client_sender = HashMap::new();
					let mut client_receiver = HashMap::new();
					for name in &self.client_registry {
						let (cs, cr) = channel();
						client_sender.insert(name.clone(), cs);
						client_receiver.insert(name.clone(), cr);
					}
					CLIENT.write().unwrap().insert(client_id);
					// start new thread for serverclient handling
					thread::spawn(move || {
						let mut client = client;
						let sleep_ms = sleep_ms.clone();
						let mut last_ping = SystemTime::now();
						let mut future_responses = vec![];
						loop {
							let now = SystemTime::now();
							// receiving calling messages from client
							match client.recv_message() {
								Ok(OwnedMessage::Text(value)) => {
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
												let future = function(params);
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
												future_responses.push(receive);
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
								Ok(OwnedMessage::Close(_)) => break,
								Ok(OwnedMessage::Ping(_)) => {
									client.send_message(&OwnedMessage::Pong(vec![])).expect("server pong");
									last_ping = now;
								},
								Err(WebSocketError::NoDataAvailable) => {},
								Err(WebSocketError::IoError(_)) => {},
								Err(err) => panic!("{}", err),
								_ => panic!("unsupported none-text type message from client")
							}
							// fetching message from server client
							if let Ok(message) = client_reader.try_recv() {
								if message == String::from("_SHUTDOWN_") {
									client.send_message(&OwnedMessage::Close(None)).expect("server client shutdown");
									break
								}
								client.send_message(&Message::text(message)).expect("send server request to client")
							}
							// check connection alive status
							if now.duration_since(last_ping).unwrap() > Duration::from_secs(8) {
								break
							}
							// handle all of future responses
							future_responses = future_responses
								.into_iter()
								.filter(|receive| {
									if let Ok(response) = receive.try_recv() {
										client.send_message(&Message::text(response)).expect("send server response to client");
										false
									} else {
										true
									}
								})
								.collect::<Vec<_>>();
							thread::sleep(Duration::from_millis(sleep_ms));
						}
						KICKED_CLIENTS.write().unwrap().push(client_id);
						println!("p2p serverclient #{} thread CLOSED", client_id);
					});
					callback(client_id, Some(client_receiver));
				}
				// receiving message from server controller
				if let Ok((client_id, message)) = reader.try_recv() {
					if client_id > 0 {
						// close specified serverclient
						if CLIENT.write().unwrap().get(&client_id).is_some() {
							serverclients
								.get(&client_id)
								.expect("get client from serverclients")
								.send(message)
								.unwrap();
						} else {
							println!("client id #{} is non-existent", client_id);
						}
					} else {
						// close all serverclients
						for client_id in &*CLIENT.write().unwrap() {
							serverclients
								.get(client_id)
								.expect("get client from serverclients")
								.send(message.clone())
								.unwrap();
						}
						skip = message == String::from("_SHUTDOWN_");
					}
				}
				// kick clients from KICKED_CLIENTS
				if !KICKED_CLIENTS.read().unwrap().is_empty() {
					let mut kicked_ids = KICKED_CLIENTS.write().unwrap();
					for client_id in &*kicked_ids {
						CLIENT.write().unwrap().remove(client_id);
						callback(client_id.clone(), None);
					}
					kicked_ids.clear();
				}
				// check skip main thread
				if skip && CLIENT.read().unwrap().is_empty() {
					break
				}
				thread::sleep(Duration::from_millis(sleep_ms));
			}
			println!("p2p server thread CLOSED");
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
		if let Ok(clients) = CLIENT.read() {
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

	pub fn set_id(&mut self, client_id: i32) {
		self.client_id = client_id;
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
