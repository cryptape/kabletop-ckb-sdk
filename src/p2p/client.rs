use websocket::{
	ClientBuilder, Message, message::OwnedMessage, result::WebSocketError
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
	collections::HashMap, thread, time::Duration, sync::mpsc::{
		channel, Receiver, Sender
	}
};
use super::Wrapper;

// a client instance connecting to server
pub struct Client {
	client_registry: HashMap<String, Box<dyn Fn(Value) -> Result<Value, String> + Send + 'static>>,
	server_registry: Vec<String>,
	socket:          String,
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
	pub fn connect(self, sleep_ms: u64) -> Result<ClientSender> {
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
		thread::spawn(move || loop {
			// receiving response and calling messages from server, client's recv_messsage method will be blocked
			match client.recv_message() {
				Ok(OwnedMessage::Text(value)) => {
					let message: Wrapper = {
						let value = from_str(value.as_str()).expect("parse server message");
						from_value(value).unwrap()
					};
					// check wether message is in the client registry table
					if let Some(callback) = self.client_registry.get(&message.name) {
						let params = from_str(message.body.as_str()).unwrap();
						let response = {
							let result = callback(params).unwrap();
							to_string(
								&json!(Wrapper {
									name: message.name,
									body: to_string(&result).unwrap()
								})
							).unwrap()
						};
						client.send_message(&Message::text(response)).expect("send client response to server");
					// check wether message is in the client request table
					} else if let Some(response) = server_sender.get(&message.name) {
						response.send(message.body).unwrap();
					} else {
						panic!("message {} isn't registered in both server and client registry table", message.name);
					}
				},
				Err(WebSocketError::NoDataAvailable) => {},
				Err(WebSocketError::IoError(_)) => {},
				Err(err) => panic!("{}", err),
				_ => panic!("unsupported none-text type message from server")
			}
			// receiving calling messages from client call
			if let Ok(message) = reader.try_recv() {
				client.send_message(&Message::text(message)).expect("send client request to server");
			}
			thread::sleep(Duration::from_millis(sleep_ms));
		});
		Ok(ClientSender::new(writer, server_receiver))
	}
}

// clientsender represents a sender feature in client to handle sending request to server
pub struct ClientSender {
	writer:          Sender<String>,
    server_response: HashMap<String, Receiver<String>>,
}

impl ClientSender {
	pub fn new(writer: Sender<String>, response: HashMap<String, Receiver<String>>) -> Self {
		ClientSender {
			writer:          writer,
			server_response: response
		}
	}

	pub fn call<T: Serialize, R: DeserializeOwned>(&mut self, name: &str, params: T) -> Result<R> {
		if let Some(response) = self.server_response.get(&String::from(name)) {
			let request = to_string(
				&json!(Wrapper {
					name: String::from(name),
					body: to_string(&json!(params))?
				})
			)?;
			self.writer.send(request)?;
			let value = {
				let value = response.recv()?;
				from_str(value.as_str())?
			};
			Ok(from_value(value)?)
		} else {
			Err(anyhow!("method {} isn't registered", name))
		}
	}
}
