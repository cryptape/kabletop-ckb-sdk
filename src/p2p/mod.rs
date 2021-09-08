use serde::{
	Serialize, Deserialize, de::DeserializeOwned
};
use anyhow::Result;

mod server;
mod client;

pub use server::{
	Server, ServerClient
};
pub use client::{
	Client, ClientSender
};
pub trait Caller {
	fn call<T: Serialize, R: DeserializeOwned>(&self, name: &str, params: T) -> Result<R>;
}

#[derive(Serialize, Deserialize)]
enum Wrapper {
	Send(Payload),
	Reply(Payload)
}

#[derive(Serialize, Deserialize)]
struct Payload {
	name: String,
	body: String
}

#[derive(Serialize, Deserialize)]
struct Error {
	reason: String
}

#[cfg(test)]
mod test {
	use super::{
		Server, Client, Caller
	};
	use serde::{
		Deserialize, Serialize
	};
	use serde_json::json;

	#[derive(Serialize)]
	struct Request {
		value: String
	}

	#[derive(Serialize, Deserialize, Debug)]
	struct Response {
		value: String
	}

	#[test]
	fn test_jsonrpc_success() {
		let server = Server::new("0.0.0.0:11525")
			.register("hello", |params| {
				Box::pin(async move {
					println!("Server => {:?}", params);
					Ok(json!(Response {
						value: String::from("hello: server responce")
					}))
				})
			})
			.register_call("world")
			.listen(300, |active| println!("server_active = {}", active))
			.unwrap();
		let client = Client::new("ws://127.0.0.1:11525")
			.register_call("hello")
			.register("world", |params| {
				Box::pin(async move {
					println!("{:?}", params);
					Ok(json!(Response {
						value: String::from("world: client response")
					}))
				})
			})
			.connect(300, || println!("client_active = false"))
			.unwrap();
		println!("1. Client to Server");
		let result: Response = client.call("hello", Request {
			value: String::from("hello: client request")
		}).unwrap();
		println!("Client => {:?}", result);
		println!("2. Server to Client");
		let result: Response = server.call("world", Request {
			value: String::from("world: server request")
		}).unwrap();
		println!("{:?}", result);
		client.shutdown();
		std::thread::sleep(std::time::Duration::from_millis(2000));
	}

	#[test]
	fn test_jsonrpc_error() {
		Server::new("0.0.0.0:11525")
			.register("hello", |params| {
				Box::pin(async move {
					println!("Server => {:?}", params);
					Err(String::from("bad hello result"))
				})
			})
			.listen(300, |_| {})
			.unwrap();
		let client = Client::new("ws://127.0.0.1:11525")
			.register_call("hello")
			.connect(300, || {})
			.unwrap();
		println!("1. Client to Server [Error]");
		let result: Response = client.call("hello", Request {
			value: String::from("hello: client request")
		}).unwrap();
		println!("Client => {:?}", result);
	}
}
