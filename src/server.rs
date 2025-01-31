use std::io::{self, BufRead, Write};
use serde_json;
use log::{debug, error, info, warn};
use zmq;
use std::sync::{Arc, mpsc};
mod utils;
use utils::{ZmqJsonCommunication, ChatMsg, input};

fn main(){
  env_logger::init();
  let zmq_ctx = Arc::new(zmq::Context::new());
  let ctx_for_rep = Arc::clone(&zmq_ctx);
  let (thread_sender, main_receiver) = mpsc::channel();
  let rep_handle = std::thread::spawn(move ||{
    debug!("Child thread with REP start");
    let socket;
    match ctx_for_rep.socket(zmq::REP) {
      Ok(_val) => {socket = _val;debug!("REP socket created");},
      Err(e) => {error!("Failed to create socket: {}", e.to_string());let _=thread_sender.send("init failed");return;}
    }
    match socket.bind("tcp://*:5555") {
      Ok(_val) => {debug!("Bind ok");},
      Err(e) => {error!("Failed to bind to 5555: {}", e.to_string());let _=thread_sender.send("init failed");return;}
    }
    info!("Listening thread ok");
    let _=thread_sender.send("init ok");
    loop {
      let msg: ChatMsg;
      match socket.recv_json(Some(0)) {
        Ok(_val) => {msg = _val;debug!("Msg received");},
        Err(e) => {
          if let Some(zmq_e) = e.downcast_ref::<zmq::Error>() {
            error!("Zmq err occured: {}", zmq_e.to_string());
            continue;
          }else if let Some(json_e) = e.downcast_ref::<serde_json::Error>(){
            error!("Json err occured: {}", json_e.to_string());
            continue;
          }
          else {
            error!("Unknow err occured: {}", e.to_string());
            continue;
          }
        }
      }
      match msg.msg_type{
        0 => {
          info!("Quit msg received, quiting...");
          break;
        }
        1 => {
          info!("Received msg to server");
          println!("{}", msg.msg);
        }
        _ => {
          warn!("Unknow msg type, drop");
        }
      }
    }
  });
  debug!("Main thread with REQ start");
  info!("Waiting for REP thread");
  let rep_state = main_receiver.recv().unwrap();
  if rep_state == "init failed" {
    error!("REP thread err detected, exiting...");
    return;
  }else {
    debug!("Receive REP thread ok");
  }
  info!("Shell ok");
  loop {
    let user_input = input("Enter command: ");
    let mut cmd_it = user_input.split_whitespace();
    let cmd_type;
    match cmd_it.next() {
      Some(_val) => {cmd_type = _val;}
      None => {
        warn!("No cmd given, try again");
        continue;
      }
    }
    match cmd_type {
      "q" => {
        let quit_msg = ChatMsg::default();
        let quit_socket = zmq_ctx.socket(zmq::REQ).unwrap();
        quit_socket.connect("tcp://127.0.0.1:5555").unwrap();
        quit_socket.send_json(&quit_msg, None).unwrap();
        break;
      }
      _ => {
        warn!("Unknow cmd");
        continue;
      }
    }
  }
  rep_handle.join().unwrap();
  info!("Total exiting...");
}

