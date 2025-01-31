use zmq;
use log::{debug, info, error};
mod utils;
use utils::{ZmqJsonCommunication, ChatMsg};

fn main() {
  env_logger::init();
  let zmq_context = zmq::Context::new();
  let requester;
  match zmq_context.socket(zmq::REQ) {
    Ok(val) => {requester = val;},
    Err(e) => {error!("failed to create socket: {}", e.to_string());return;}
  }
  match requester.connect("tcp://127.0.0.1:5555") {
    Ok(_val) => {info!("connect to 5555 successfully");},
    Err(e) => {error!("failed to connect 5555: {}", e.to_string());return;}
  }
  let msg = "Hello";
  match requester.send(msg.as_bytes(), 0) {
    Ok(_val) => {debug!("msg {} sent", msg);},
    Err(e) => {error!("failed to send {}: {}", msg, e.to_string());return;}
  }
  let mut ret_msg = zmq::Message::new().unwrap();
  match requester.recv(&mut ret_msg, 0) {
    Ok(_val) => {debug!("recv msg");},
    Err(e) => {error!("failed recv msg: {}", e.to_string());return;}
  }
  let ret_msg_str;
  match ret_msg.as_str() {
    Some(val) => {ret_msg_str = val;},
    None => {error!("failed convert msg");return;}
  }
  println!("{}", ret_msg_str);
}
