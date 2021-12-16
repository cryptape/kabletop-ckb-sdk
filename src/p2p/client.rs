use websocket::{
	ClientBuilder, message::OwnedMessage, sender::Writer
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
	collections::HashMap, thread, net::TcpStream, time::{
		Duration, SystemTime
	}, sync::{
		RwLock, mpsc::{
			channel, Receiver, Sender
		}
	}
};
use super::{
	Wrapper, Payload, Error, Caller
};
use futures::{
	future::BoxFuture, executor::block_on
};

lazy_static! {
	static ref HEARTBEAT: RwLock<(SystemTime, SystemTime)> = RwLock::new((SystemTime::now(), SystemTime::now()));
	static ref SERVER: RwLock<Option<Writer<TcpStream>>> = RwLock::new(None);
	static ref CALLBACK: RwLock<Option<Box<dyn Fn() + Send + Sync + 'static>>> = RwLock::new(None);
}

fn callback() {
	if let Some(cb) = &*CALLBACK.read().unwrap() {
		cb();
	}
}

fn close_server() {
	if SERVER.write().unwrap().is_some() {
		callback();
	}
	*SERVER.write().unwrap() = None;
}

fn server_send(msg: OwnedMessage) {
	let mut ok = true;
	if let Some(server) = &mut *SERVER.write().unwrap() {
		if let OwnedMessage::Close(_) = msg {
			ok = false;
		}
		if let Err(err) = server.send_message(&msg) {
			println!("clien send error => {}", err);
			ok = false;
		}
	}
	if !ok {
		close_server();
	}
}

fn update_heartbeat(ping: Option<SystemTime>, pong: Option<SystemTime>) {
	let (mut last_ping, last_pong) = *HEARTBEAT.write().unwrap();
	if ping.is_some() {
		last_ping = ping.unwrap();
		*HEARTBEAT.write().unwrap() = (ping.unwrap(), last_pong);
	}
	if pong.is_some() {
		*HEARTBEAT.write().unwrap() = (last_ping, pong.unwrap());
	}
}

// a client instance connecting to server
pub struct Client {
	client_registry: HashMap<String, Box<dyn Fn(i32, Value) -> BoxFuture<'static, Result<Value, String>> + Send + 'static>>,
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
			F: Fn(i32, Value) -> BoxFuture<'static, Result<Value, String>> + Send + 'static
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
	pub fn connect<F>(self, sleep_ms: u64, local_callback: F) -> Result<ClientSender> 
		where
			F: Fn() + Send + Sync + 'static
	{
		let client = ClientBuilder::new(self.socket.as_str())?.connect_insecure()?;
		let (mut stream, sink) = client.split()?;
		*SERVER.write().unwrap() = Some(sink);
		*CALLBACK.write().unwrap() = Some(Box::new(local_callback));
		let mut server_sender = HashMap::new();
		let mut server_receiver = HashMap::new();
		for name in &self.server_registry {
			let (ss, sr) = channel();
			server_sender.insert(name.clone(), ss);
			server_receiver.insert(name.clone(), sr);
		}
		let (writer, reader) = channel();
		// start client write thread
		thread::spawn(move || {
			let sleep_ms = sleep_ms.clone();
			update_heartbeat(Some(SystemTime::now()), Some(SystemTime::now()));
			loop {
				if SERVER.read().unwrap().is_none() {
					println!("p2p client thread CLOSED");
					return
				}
				let (last_ping, last_pong) = *HEARTBEAT.read().unwrap();
				// receiving calling messages from client call
				if let Ok(message) = reader.try_recv() {
					if message == String::from("_SHUTDOWN_") {
						server_send(OwnedMessage::Close(None));
					} else {
						server_send(OwnedMessage::Text(message));
					}
				}
				// check connection alive status
				if SystemTime::now().duration_since(last_pong).unwrap() > Duration::from_secs(8) {
					close_server();
				}
				// check sending heartbeat message
				if SystemTime::now().duration_since(last_ping).unwrap() > Duration::from_secs(2) {
					server_send(OwnedMessage::Ping(vec![]));
					update_heartbeat(Some(SystemTime::now()), None);
				}
				thread::sleep(Duration::from_millis(sleep_ms));
			}
		});
		// start client read thread
		thread::spawn(move || {
			let sleep_ms = sleep_ms.clone();
			loop {
				if SERVER.read().unwrap().is_none() {
					println!("p2p client WORKER thread CLOSED");
					return
				}
				// receiving response and calling messages from server, client's recv_messsage method will be blocked
				let recv = stream.recv_message();
				if let Err(err) = recv {
					println!("client error => {:?}", err);
					close_server();
					continue
				}
				match recv.unwrap() {
					OwnedMessage::Text(value) => {
						let message: Wrapper = {
							let value = from_str(value.as_str()).expect("parse server message");
							from_value(value).unwrap()
						};
						match message {
							Wrapper::Send(payload) => {
								// check wether message is in the client registry table
								if let Some(function) = self.client_registry.get(&payload.name) {
									let params = from_str(payload.body.as_str()).unwrap();
									let future = function(0, params);
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
										server_send(OwnedMessage::Text(response));
									});
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
					OwnedMessage::Close(_) => close_server(),
					OwnedMessage::Pong(_) => update_heartbeat(None, Some(SystemTime::now())),
					_ => panic!("unsupported none-text type message from server")
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
		if let Err(err) = self.writer.send(String::from("_SHUTDOWN_")) {
			println!("[WARN] shutdown error: {}", err);
		}
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
