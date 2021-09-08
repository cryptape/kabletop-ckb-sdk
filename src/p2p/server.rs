use websocket::{
	Message, sync::Server as WsServer, message::OwnedMessage, result::WebSocketError
};
use std::{
	thread, net::SocketAddr, collections::HashMap, time::{
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
	static ref ACTIVE: RwLock<bool> = RwLock::new(false);
}

// a server instance for handling registering both request/response methods and listening at none-blocking mode,
// but only supports one connection at the same time
pub struct Server {
	server_registry: HashMap<String, Box<dyn Fn(Value) -> BoxFuture<'static, Result<Value, String>> + Send + 'static>>,
	client_registry: Vec<String>,
	socket:          SocketAddr
}

impl Server {
	pub fn new(socket: &str) -> Self {
		Server {
			server_registry: HashMap::new(),
			client_registry: Vec::new(),
			socket:          socket.parse().expect("parse socket string")
		}
	}

	// register a function instance to respond client request
	pub fn register<F>(mut self, name: &str, method: F) -> Self
		where
			F: Fn(Value) -> BoxFuture<'static, Result<Value, String>> + Send + 'static
	{
		self.server_registry.insert(String::from(name), Box::new(method));
		self
	}

	// register a function name that enables server to send request to client
	pub fn register_call(mut self, name: &str) -> Self {
		self.client_registry.push(String::from(name));
		self
	}

	// listen connections at none-blocking mode
	pub fn listen<F>(self, sleep_ms: u64, callback: F) -> Result<ServerClient> 
		where
			F: Fn(bool) + Send + 'static
	{
		let mut server = WsServer::bind(self.socket)?;
		server.set_nonblocking(true)?;
		let mut client_sender = HashMap::new();
		let mut client_receiver = HashMap::new();
		for name in &self.client_registry {
			let (cs, cr) = channel();
			client_sender.insert(name.clone(), cs);
			client_receiver.insert(name.clone(), cr);
		}
		let (writer, reader) = channel();
		thread::spawn(move || {
			let mut client;
			loop {
				if let Ok(connect) = server.accept() {
					client = connect.accept().expect("accept connection");
					client.set_nonblocking(true).expect("set blocking");
					*ACTIVE.write().unwrap() = true;
					callback(true);
					break
				}
				if let Ok(message) = reader.try_recv() {
					if message == String::from("_SHUTDOWN_") {
						*ACTIVE.write().unwrap() = false;
						callback(false);
						return
					}
				}
				thread::sleep(Duration::from_millis(sleep_ms));
			}
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
								if let Some(function) = self.server_registry.get(&payload.name) {
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
				if let Ok(message) = reader.try_recv() {
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
			*ACTIVE.write().unwrap() = false;
			callback(false);
			println!("p2p server thread CLOSED");
		});
		Ok(ServerClient::new(writer, client_receiver))
	}
}

// serverclient representing one connecting which generated after the server accepted one client
// to handle request from server to that client
pub struct ServerClient {
	writer:          Sender<String>,
	client_response: HashMap<String, Receiver<String>>
}

impl ServerClient {
	pub fn new(writer: Sender<String>, response: HashMap<String, Receiver<String>>) -> Self {
		ServerClient {
			writer:          writer,
			client_response: response
		}
	}

	pub fn active(&self) -> bool {
		if let Ok(active) = ACTIVE.read() {
			*active
		} else {
			false
		}
	}

	pub fn shutdown(&self) {
		self.writer
			.send(String::from("_SHUTDOWN_"))
			.expect("send shutdown");
	}
}

impl Caller for ServerClient {
	fn call<T: Serialize, R: DeserializeOwned>(&self, name: &str, params: T) -> Result<R> {
		if !self.active() {
			return Err(anyhow!("no client connected"));
		}
		if let Some(receiver) = self.client_response.get(&String::from(name)) {
			let request = to_string(
				&Wrapper::Send(
					Payload {
						name: String::from(name),
						body: to_string(&json!(params))?
					}
				)
			)?;
			self.writer.send(request)?;
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
	}
}
