use serde::{
	Serialize, Deserialize
};

mod server;
mod client;

pub use server::{
	Server, ServerClient
};
pub use client::{
	Client, ClientSender
};

#[derive(Serialize, Deserialize)]
struct Wrapper {
	name: String,
	body: String
}

#[cfg(test)]
mod test {
	use super::{
		Server, Client
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
	fn test_jsonrpc() {
		let mut server = Server::new("0.0.0.0:11525")
			.register("hello", |params| {
				println!("Server => {:?}", params);
				Ok(json!(Response {
					value: String::from("hello: server responce")
				}))
			})
			.register_call("world")
			.listen(300)
			.unwrap();
		let mut client = Client::new("ws://127.0.0.1:11525")
			.register_call("hello")
			.register("world", |params| {
				println!("{:?}", params);
				Ok(json!(Response {
					value: String::from("world: client response")
				}))
			})
			.connect(300)
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
	}
}
