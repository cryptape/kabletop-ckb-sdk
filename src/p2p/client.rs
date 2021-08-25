use websocket::{
	ClientBuilder, Message, message::OwnedMessage, result::WebSocketError, ws, sync::Client as WsClient
};
use serde_json::{
    from_value, json, Value, to_string, from_str
};
use serde::{
	Serialize, de::DeserializeOwned
};
use anyhow::{
    Result, anyhow
};
use std::{
	collections::HashMap, thread, net::TcpStream, io::ErrorKind, time::{
		Duration, SystemTime
	}, sync::mpsc::{
		channel, Receiver, Sender
	}
};
use super::{
	Wrapper, Payload, Error, Caller
};

// a client instance connecting to server
pub struct Client {
	client_registry: HashMap<String, Box<dyn Fn(Value) -> Result<Value, String> + Send + 'static>>,
	server_registry: Vec<String>,
	socket:          String
}

impl Client {
	pub fn new(socket: &str) -> Self {
		Client {
			client_registry: HashMap::new(),
			server_registry: Vec::new(),
			socket:          String::from(socket)
		}
	}

	// register a function to respond server request
	pub fn register<F>(mut self, name: &str, method: F) -> Self
		where
			F: Fn(Value) -> Result<Value, String> + Send + 'static
	{
		self.client_registry.insert(String::from(name), Box::new(method));
		self
	}

	// register a function name that enables client to send request to server
	pub fn register_call(mut self, name: &str) -> Self {
		self.server_registry.push(String::from(name));
		self
	}

	// connect to server and listen request from server
	pub fn connect<F>(self, sleep_ms: u64, callback: F) -> Result<ClientSender> 
		where
			F: Fn() + Send + 'static
	{
		let mut client = ClientBuilder::new(self.socket.as_str())?.connect_insecure()?;
		client.set_nonblocking(true)?;
		let mut server_sender = HashMap::new();
		let mut server_receiver = HashMap::new();
		for name in &self.server_registry {
			let (ss, sr) = channel();
			server_sender.insert(name.clone(), ss);
			server_receiver.insert(name.clone(), sr);
		}
		let (writer, reader) = channel();
		thread::spawn(move || {
			fn _send<M: ws::Message>(client: &mut WsClient<TcpStream>, msg: M, callback: &dyn Fn()) -> bool {
				match client.send_message(&msg) {
					Ok(_) => true,
					Err(WebSocketError::IoError(err)) => {
						if err.kind() == ErrorKind::ConnectionReset {
							callback();
							false
						} else {
							panic!("client send error: {}", err);
						}
					},
					Err(err) => Err(err).expect("client send")
				}
			}
			let mut last_pong = SystemTime::now();
			let mut last_ping = SystemTime::now();
			loop {
				let now = SystemTime::now();
				// receiving response and calling messages from server, client's recv_messsage method will be blocked
				match client.recv_message() {
					Ok(OwnedMessage::Text(value)) => {
						let message: Wrapper = {
							let value = from_str(value.as_str()).expect("parse server message");
							from_value(value).unwrap()
						};
						match message {
							Wrapper::Send(payload) => {
								// check wether message is in the client registry table
								if let Some(function) = self.client_registry.get(&payload.name) {
									let params = from_str(payload.body.as_str()).unwrap();
									let response = {
										let body: String;
										match function(params) {
											Ok(result)  => body = to_string(&result).unwrap(),
											Err(reason) => body = to_string(&json!(Error { reason })).unwrap()
										}
										to_string(
											&Wrapper::Reply(Payload { name: payload.name, body })
										).unwrap()
									};
									if !_send(&mut client, Message::text(response), &callback) {
										break
									}
								} else {
									panic!("message {} isn't registered in server registry table", payload.name);
								}
							},
							Wrapper::Reply(payload) => {
								// check wether message is in the client request table
								if let Some(response) = server_sender.get(&payload.name) {
									response.send(payload.body).unwrap();
								} else {
									panic!("message {} isn't registered in client registry table", payload.name);
								}
							}
						}
					},
					Ok(OwnedMessage::Close(_)) => {
						callback();
						break
					},
					Ok(OwnedMessage::Pong(_)) => last_pong = now,
					Err(WebSocketError::NoDataAvailable) => {},
					Err(WebSocketError::IoError(_)) => {},
					Err(err) => panic!("{}", err),
					_ => panic!("unsupported none-text type message from server")
				}
				// receiving calling messages from client call
				if let Ok(message) = reader.try_recv() {
					if message == String::from("_SHUTDOWN_") {
						if !_send(&mut client, OwnedMessage::Close(None), &callback) {
							break
						}
					} else {
						if !_send(&mut client, Message::text(message), &callback) {
							break
						}
					}
				}
				// check connection alive status
				if now.duration_since(last_pong).unwrap() > Duration::from_secs(8) {
					callback();
					break
				}
				// check sending heartbeat message
				if now.duration_since(last_ping).unwrap() > Duration::from_secs(2) {
					if !_send(&mut client, OwnedMessage::Ping(vec![]), &callback) {
						break
					}
					last_ping = now;
				}
				thread::sleep(Duration::from_millis(sleep_ms));
			}
		});
		Ok(ClientSender::new(writer, server_receiver))
	}
}

// clientsender represents a sender feature in client to handle sending request to server
pub struct ClientSender {
	writer:          Sender<String>,
    server_response: HashMap<String, Receiver<String>>
}

impl ClientSender {
	pub fn new(writer: Sender<String>, response: HashMap<String, Receiver<String>>) -> Self {
		ClientSender {
			writer:          writer,
			server_response: response
		}
	}

	pub fn shutdown(&self) {
		self.writer
			.send(String::from("_SHUTDOWN_"))
			.expect("send shutdown");
	}
}

impl Caller for ClientSender {
	fn call<T: Serialize, R: DeserializeOwned>(&self, name: &str, params: T) -> Result<R> {
		if let Some(response) = self.server_response.get(&String::from(name)) {
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
				let value: Value = from_str(response.recv()?.as_str())?;
				if let Ok(error) = from_value::<Error>(value.clone()) {
					return Err(anyhow!("error from server: {}", error.reason));
				}
				from_value(value)?
			};
			Ok(value)
		} else {
			Err(anyhow!("method {} isn't registered", name))
		}
	}
}
