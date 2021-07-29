use async_jsonrpc_client::{
    WsClient, Output, Transport, Params
};
use serde_json::{
    from_value, json, Value
};
use serde::{
	Serialize, de::DeserializeOwned
};
use anyhow::{
    Result, anyhow
};
use futures::executor::block_on;

pub struct Client {
	client: WsClient
}

impl Client {
	pub fn new(socket: &str) -> Self {
		Client {
			client: block_on(WsClient::new(socket)).expect("connect ws rpc server")
		}
	}

	pub fn call<T: Serialize, R: DeserializeOwned>(&self, name: &str, params: T) -> Result<R> {
		if let Value::Object(params) = json!(params) {
			let output = block_on(self.client.request(name, Some(Params::Map(params))))?;
			match output {
				Output::Success(value) => Ok(from_value(value.result)?),
				Output::Failure(value) => Err(anyhow!(value))
			}
		} else {
			panic!("bad json map object");
		}
	}
}
