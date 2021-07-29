mod server;
mod client;

pub use server::Server;
pub use client::Client;

#[cfg(test)]
mod test {
	use super::{
		Server, Client
	};
	use serde::{
		Deserialize, Serialize
	};
	use serde_json::{
		json, Value
	};
	use jsonrpc_ws_server::jsonrpc_core::{
		Error, Params
	};

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
		let greetings = |params: Params| {
			println!("{:?}", params);
			futures::future::ok::<Value, Error>(json!(Response {
				value: String::from("yes, my lord")
			}))
		};
		Server::new("0.0.0.0:11525")
			.register("greetings", greetings)
			.start();
		let client = Client::new("ws://127.0.0.1:11525");
		let result: Response = client.call("greetings", Request {
			value: String::from("hello, world")
		}).expect("calling greetings");
		println!("{:?}", result);
	}
}
