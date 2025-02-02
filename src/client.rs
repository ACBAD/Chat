use chrono::Utc;
use zmq;
use std::sync::{mpsc, Arc, OnceLock, atomic::{AtomicU8, Ordering}};
use serde::{Serialize, Deserialize};
use log::{debug, info, error, warn};
mod utils;
use utils::{ZmqJsonClient, ContactProtocol, input, NotifyProtocol, MsgStatus, MessageType, Protocols};

static CLIENT_ID: OnceLock<String> = OnceLock::new();

impl ZmqJsonClient for zmq::Socket {
  fn recv_json<Protocols: for<'a> Deserialize<'a>>(&self, flags: Option<i32>) -> Result<Protocols, Box<dyn std::error::Error>> {
    let raw_msg = self.recv_bytes(flags.unwrap_or(0))?;
    let msg = serde_json::from_slice(&raw_msg)?;
    Ok(msg)
  }
  fn send_json<Protocols: Serialize>(&self, data: &Protocols, flags: Option<i32>) -> Result<(), Box<dyn std::error::Error>> {
    let json_bytes = serde_json::to_vec(data)?;
    self.send(&json_bytes, flags.unwrap_or(0))?;
    Ok(())
  }
}

fn main() {
  env_logger::init();
  CLIENT_ID.set(input("Enter client_id: ")).unwrap();
  println!("Your client_id: {}", CLIENT_ID.get().unwrap());
  let zmq_ctx = Arc::new(zmq::Context::new());
  let ctx_for_dealer = Arc::clone(&zmq_ctx);
  let (main_sender, thread_receiver) = mpsc::channel();
  let thread_state = Arc::new(AtomicU8::new(0));
  let thread_state_clone = thread_state.clone();
  let dealer_handle = std::thread::spawn(move ||{
    debug!("Child thread with DEALER start");
    let socket;
    let thread_err = |err_msg: String, exit_code: u8|{error!("{}", err_msg);thread_state_clone.store(exit_code, Ordering::Relaxed);};
    match ctx_for_dealer.socket(zmq::DEALER) {
      Ok(_val) => {socket = _val;debug!("DEALER socket created");},
      Err(e) => {thread_err(format!("Failed to create socket: {}", e.to_string()), 9);return;}
    }
    match socket.set_identity(CLIENT_ID.get().unwrap().as_bytes()) {
      Ok(_) => {debug!("Identity set");},
      Err(e) => {thread_err(format!("Failed to set Identity due to {}", e.to_string()), 10);return;}
    }
    match socket.connect("tcp://39.105.24.101:23") {
      Ok(_val) => {debug!("Connect ok");},
      Err(e) => {thread_err(format!("Failed to bind to 5555: {}", e.to_string()), 2);return;}
    }
    match socket.set_rcvtimeo(5000) {
      Ok(_) => {debug!("Set init recv timeout ok");},
      Err(e) => {thread_err(format!("Failed to set init recv timeout: {}", e.to_string()), 3);return;}
    }
    info!("Socket connected, try to register client");
    let register_msg = 
      Protocols::CPType(ContactProtocol::ClientControl { state: MsgStatus::SUBMITTED, command: "register".to_string(), cmd_args: None, time: Utc::now() });
    match socket.send_json(&register_msg, Some(0)) {
      Ok(_val) => {debug!("Register request sent");},
      Err(e) => {thread_err(format!("Failed send register request: {}", e.to_string()),4);return;}
    }
    match socket.recv_json(Some(0)) {
      Ok(val) => {
        if let Protocols::CPType(val2) = val{
          match val2 {
            ContactProtocol::ClientControl { state, command, cmd_args, time } => {
              if state != MsgStatus::ACCEPTED{
                thread_err(format!("Register request failed, server returns {}, echo is {}", state, command), 5);
                return;
              }
              info!("Register successfully");
            },
            _ =>{
              error!("Failed deserialize register result, not other protocol");
              thread_state_clone.store(6, Ordering::Relaxed);
              return;
            }
          }
        }
        else {
          error!("Failed deserialize register result, not ContactProtocol");
          thread_state_clone.store(7, Ordering::Relaxed);
          return;
        }
      },
      Err(e) => {thread_err(format!("Failed received register result: {}", e.to_string()), 8);return;}
    }
    match socket.set_rcvtimeo(100) {
      Ok(_) => {debug!("set regular recv timeout");},
      Err(e) => {thread_err(format!("Failed set regular recv timeout: {}", e.to_string()), 11);return;}
    }
    info!("Listening thread ok");
    thread_state_clone.store(1, Ordering::Relaxed);
    loop {
      let cmd: String = thread_receiver.try_recv().unwrap_or("".to_string());
      if !cmd.is_empty(){
        match cmd.as_str() {
          "shutdown" => {
            debug!("DEALER thread exit");
            let quit_msg = 
              Protocols::CPType(ContactProtocol::ClientControl { state: MsgStatus::SUBMITTED, command: "unregister".to_string(), cmd_args: None, time: Utc::now() });
            socket.send_json(&quit_msg, Some(0))
              .unwrap_or_else(|e|{warn!("Error {} occured during say goodbye", e.to_string())});
            break;
          },
          _ => {warn!("DEALER received Unknow cmd");}
        }
      }
      let raw_msg: Protocols;
      match socket.recv_json(Some(0)) {
        Ok(_val) => {raw_msg = _val;debug!("Msg received");},
        Err(e) => {
          if let Some(zmq_e) = e.downcast_ref::<zmq::Error>() {
            if zmq_e.eq(&zmq::Error::EAGAIN){continue;}
            error!("Zmq err occured: {}", zmq_e.to_string());
          }else if let Some(json_e) = e.downcast_ref::<serde_json::Error>(){
            error!("Json err occured: {}", json_e.to_string());
          }
          else if let Some(str_e) = e.downcast_ref::<std::string::FromUtf8Error>(){
            error!("String err occured: {}", str_e.to_string());
          }
          else {
            error!("Unknow err occured: {}", e.to_string());
          }
          continue;
        }
      }
      match raw_msg {
        Protocols::CPType(_val) => {},
        Protocols::NPType(_val) => {},
      }
    }
  });

  info!("Waiting for DEALER thread");
  loop {
    let val = thread_state.load(Ordering::Relaxed);
    if val != 0{
      if val != 1{
        error!("DEALER thread err detected, exiting...");
        return;
      }
      else {
        debug!("Receive DEALER thread ok");
        break;
      }
    }
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
        info!("Shutdown cmd received, quiting...");
        main_sender.send("shutdown".to_string()).unwrap();
        break;
      },
      "send" => {
        
      },
      _ => {
        warn!("Unknow cmd");
        continue;
      }
    }
  }
  info!("Main thread done, waiting DEALER thread...");
  dealer_handle.join().unwrap();
  info!("Total exiting...");
}
