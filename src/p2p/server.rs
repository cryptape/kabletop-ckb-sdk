use jsonrpc_ws_server::{
	ServerBuilder, CloseHandle
};
use jsonrpc_ws_server::jsonrpc_core::{
	IoHandler, RpcMethodSimple
};
use std::{
	thread, net::SocketAddr
};

pub struct Server {
	io:     IoHandler,
	socket: SocketAddr
}

impl Server {
	pub fn new(socket: &str) -> Self {
		Server {
			io:     IoHandler::new(),
			socket: socket.parse().expect("parse socket string")
		}
	}

	pub fn register<F: RpcMethodSimple>(mut self, name: &str, method: F) -> Self {
		self.io.add_method(name, method);
		self
	}

	pub fn start(self) -> (CloseHandle, thread::JoinHandle<()>) {
		let server = ServerBuilder::new(self.io)
			.start(&self.socket)
			.expect("prepare ws rpc server");
		(
			server.close_handle(),
			thread::spawn(move || {
				server.wait().expect("start ws rpc server")
			})
		)
	}
}
