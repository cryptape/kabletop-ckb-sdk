use websocket::{
	Message, sync::Server as WsServer, message::OwnedMessage, result::WebSocketError
};
use std::{
	thread, net::SocketAddr, collections::HashMap, time::Duration,
	sync::mpsc::{
		Sender, Receiver, channel
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
	Wrapper, Error, Caller
};

// a server instance for handling registering both request/response methods and listening at none-blocking mode,
// but only supports one connection at the same time
pub struct Server {
	server_registry: HashMap<String, Box<dyn Fn(Value) -> Result<Value, String> + Send + 'static>>,
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
			F: Fn(Value) -> Result<Value, String> + Send + 'static
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
	pub fn listen(self, sleep_ms: u64) -> Result<ServerClient> {
		let server = WsServer::bind(self.socket)?;
		let mut client_sender = HashMap::new();
		let mut client_receiver = HashMap::new();
		for name in &self.client_registry {
			let (cs, cr) = channel();
			client_sender.insert(name.clone(), cs);
			client_receiver.insert(name.clone(), cr);
		}
		let (writer, reader) = channel();
		thread::spawn(move || {
			for conn in server.filter_map(Result::ok) {
				let mut client = conn.accept().expect("accept connection");
				client.set_nonblocking(true).expect("set blocking");
				loop {
					// receiving calling messages from client, server's recv_message won't be blocked
					match client.recv_message() {
						Ok(OwnedMessage::Text(value)) => {
							let message: Wrapper = {
								let value = from_str(value.as_str()).expect("parse client message");
								from_value(value).unwrap()
							};
							// searching in server response registry table
							if let Some(callback) = self.server_registry.get(&message.name) {
								let params = from_str(message.body.as_str()).unwrap();
								let response = {
									let body: String;
									match callback(params) {
										Ok(result)  => body = to_string(&result).unwrap(),
										Err(reason) => body = to_string(&json!(Error { reason })).unwrap()
									}
									to_string(&json!(Wrapper { name: message.name, body })).unwrap()
								};
								client.send_message(&Message::text(response)).expect("send server response to client")
							// searching in client message registry sender table
							} else if let Some(sender) = client_sender.get(&message.name) {
								sender.send(message.body).unwrap();
							} else {
								panic!("method {} can't find in both server and client registry table", message.name);
							}
						},
						Err(WebSocketError::NoDataAvailable) => {},
						Err(WebSocketError::IoError(_)) => {},
						Err(err) => panic!("{}", err),
						_ => panic!("unsupported none-text type message from client")
					}
					// fetching message from server client
					if let Ok(message) = reader.try_recv() {
						client.send_message(&Message::text(message)).expect("send server request to client")
					}
					thread::sleep(Duration::from_millis(sleep_ms));
				}
			}
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
}

impl Caller for ServerClient {
	fn call<T: Serialize, R: DeserializeOwned>(&mut self, name: &str, params: T) -> Result<R> {
		if let Some(receiver) = self.client_response.get(&String::from(name)) {
			let request = to_string(
				&Wrapper {
					name: String::from(name),
					body: to_string(&json!(params))?
				}
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
