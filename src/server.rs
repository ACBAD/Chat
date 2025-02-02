use chrono::{DateTime, Utc};
use serde_json;
use serde::{Deserialize, Serialize};
use log::{debug, error, info, warn};
use zmq;
use std::{collections::HashMap, i8, ptr::eq, sync::{mpsc, Arc, Mutex, OnceLock}};
mod utils;
use utils::{ZmqJsonServer, ContactProtocol, input, MsgStatus, NotifyProtocol, MessageType, Protocols};

impl ZmqJsonServer for zmq::Socket {
  fn recv_json<Protocols: for<'a> Deserialize<'a>>(&self, flags: Option<i32>) -> Result<(String, Protocols), Box<dyn std::error::Error>> {
    let raw_msgs = self.recv_multipart(flags.unwrap_or(0))?;
    debug!("id: {:?}", raw_msgs[0]);
    let client_id = String::from_utf8(raw_msgs[0].to_vec())?;
    let msg = serde_json::from_slice(&raw_msgs[1])?;
    Ok((client_id, msg))
  }
  fn send_json<Protocols: Serialize>(&self, client_id: &String, data: &Protocols, flags: Option<i32>) -> Result<(), Box<dyn std::error::Error>> {
    let json_bytes = serde_json::to_vec(data)?;
    let client_id_bytes = client_id.as_bytes();
    self.send_multipart(&[&client_id_bytes, &json_bytes], flags.unwrap_or(0))?;
    Ok(())
  }
}

struct Client{
  state: i8,
  login_time: DateTime<Utc>,
  client_id: String,
}

trait ClientMethods {
  fn respond(&self, socket: &zmq::Socket, state: MsgStatus, command: String, cmd_args: Option<serde_json::Value>) -> Result<(), Box<dyn std::error::Error>>;
  fn notify(&self, socket: &zmq::Socket, msg: NotifyProtocol) -> Result<(), Box<dyn std::error::Error>>;
}

impl ClientMethods for Client {
  fn respond(&self, socket: &zmq::Socket, state: MsgStatus, command: String, cmd_args: Option<serde_json::Value>) -> Result<(), Box<dyn std::error::Error>> {
    let reponse_msg;
    reponse_msg = Protocols::CPType(ContactProtocol::ClientControl { state: state, command: command, cmd_args: cmd_args, time: Utc::now() });
    debug!("Respond to {}", self.client_id);
    socket.send_json(&self.client_id, &reponse_msg, Some(0))
  }
  fn notify(&self, socket: &zmq::Socket, action: NotifyProtocol) -> Result<(), Box<dyn std::error::Error>> {
    match action {
      NotifyProtocol::MsgFromUser { ref sender, ref content } => {
        socket.send_json(&self.client_id, &action, Some(0))
      }
    }
    
  }
}

static CLIENTS: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();

fn get_clients() -> &'static Mutex<HashMap<String, Client>> {
  CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn main(){
  env_logger::init();
  let zmq_ctx = Arc::new(zmq::Context::new());
  let ctx_for_router = Arc::clone(&zmq_ctx);
  let (thread_sender, main_receiver) = mpsc::channel();
  get_clients().lock().unwrap().insert("root".to_string(), Client {state: 0, login_time: Utc::now(), client_id: "root".to_string()});
  let router_handle = std::thread::spawn(move ||{
    debug!("Child thread with ROUTER start");
    let socket;
    match ctx_for_router.socket(zmq::ROUTER) {
      Ok(_val) => {socket = _val;debug!("ROUTER socket created");},
      Err(e) => {error!("Failed to create socket: {}", e.to_string());let _=thread_sender.send("init failed");return;}
    }
    match socket.bind("tcp://*:23") {
      Ok(_val) => {debug!("Bind ok");},
      Err(e) => {error!("Failed to bind to 5555: {}", e.to_string());let _=thread_sender.send("init failed");return;}
    }
    info!("Listening thread ok");
    let _=thread_sender.send("init ok");
    loop {
      let raw_msg;
      let client_id: String;
      match socket.recv_json(Some(0)) {
        Ok(_val) => {(client_id, raw_msg) = _val;debug!("Msg received");},
        Err(e) => {
          if let Some(zmq_e) = e.downcast_ref::<zmq::Error>() {
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
      if let Protocols::CPType(ContactProtocol::ClientControl { ref state, ref command, ref cmd_args, ref time }) = raw_msg{
        if command == "register"{
          if let Some(client) = get_clients().lock().unwrap().get(&client_id){
            warn!("Client {} has registered, reject another registry", client_id);
            client.respond(&socket, MsgStatus::REJECTED, "Multiple registry".to_string(), None)
              .unwrap_or_else(|e|{warn!("Error occured during respond to client: {}", e.to_string());});
            continue;
          }
          info!("New client connect: {}", client_id);
          let mut clients_lock = get_clients().lock().unwrap();
          let this_client = Client {state: 0, login_time: Utc::now(), client_id: client_id.clone()};
          clients_lock.insert(client_id.clone(), this_client);
          clients_lock.get(&client_id).unwrap().respond(&socket, MsgStatus::ACCEPTED, command.clone(), cmd_args.clone())
            .unwrap_or_else(|e|{error!("Error occured during confirm register: {}", e.to_string());});
          continue;
        }
      }
      if !get_clients().lock().unwrap().contains_key(&client_id){
        warn!("Not registered client: {}", client_id);
        let reject_msg = ContactProtocol::ClientControl { state: MsgStatus::REJECTED, command: "register".to_string(), cmd_args: None, time: Utc::now() };
        socket.send_json(&client_id, &reject_msg, Some(0))
          .unwrap_or_else(|e|{error!("Error {} occured during reject unregistered client {}", e.to_string(), client_id);});
      }
      match raw_msg{
        Protocols::CPType(ContactProtocol::ServerControl { state, command, cmd_args, time })=> {
          debug!("Receive server cmd from {}", client_id);
          if client_id != "root" {
            warn!("Client {} try to use server cmd", client_id);
            continue;
          }
          match command.as_str() {
            "shutdown" => {
              info!("Shutdown cmd received, quiting...");
              break;
            },
            _ => {
              warn!("invalid server cmd");
              continue;
            }
          }
        },
        Protocols::CPType(ContactProtocol::ClientControl { state, command, cmd_args, time }) => {
          debug!("ClientControl command {} has been requested", command);
          let mut clients_lock = get_clients().lock().unwrap();
          debug!("ClientControl get lock");
          let this_client = clients_lock.get(client_id.as_str()).unwrap();
          let respond_failed_callback = 
            |e: Box<dyn std::error::Error>, cmd: String|{error!("Error {} occured during responding {} for {}", e.to_string(), client_id, cmd);};
          match command.as_str() {
            "get_clients" => {
              let mut clients_vec = Vec::<String>::new();
              for client in clients_lock.iter(){
                clients_vec.push(client.0.to_string());
              }
              let clients_json = serde_json::to_value(clients_vec).unwrap();
              this_client.respond(&socket, MsgStatus::ACCEPTED, "get_clients".to_string(), Some(clients_json))
                .unwrap_or_else(|e|{respond_failed_callback(e, command);});
            }
            "unregister" => {
              clients_lock.remove(&client_id).unwrap();
              info!("Client {} gone", client_id);
            }
            _ => {warn!("Client {} send invalid client cmd", client_id);}
          }
        },
        Protocols::CPType(ContactProtocol::User2UserMsg { state, target, content, time }) => {
          let clients_lock = get_clients().lock().unwrap();
          let target_client = clients_lock.get(&target);
          let this_client = clients_lock.get(&client_id).unwrap();
          if target_client.is_none(){
            this_client.respond(&socket, MsgStatus::FAILED, "No such target".to_string(), None)
              .unwrap_or_else(|e|{error!("Error {} occured during respond {}'s TextMsg", e.to_string(), client_id)});
            continue;
          }
          target_client.unwrap().notify(&socket, NotifyProtocol::MsgFromUser { sender: client_id, content: content })
            .unwrap_or_else(|e|{error!("Error {} occured during notify {}", e.to_string(), target)});
        }
        Protocols::NPType(_) => {
          warn!("Wrong type from {}!", client_id);
        } 
      }
    }
  });
  info!("Waiting for ROUTER thread");
  let rep_state = main_receiver.recv().unwrap();
  if rep_state == "init failed" {
    error!("ROUTER thread err detected, exiting...");
    return;
  }else {
    debug!("Receive ROUTER thread ok");
  }
  let control_socket = zmq_ctx.socket(zmq::DEALER).unwrap();
  control_socket.set_identity("root".as_bytes()).unwrap();
  control_socket.connect("tcp://localhost:23").unwrap();
  let send_control = |socket: &zmq::Socket, control_msg: ContactProtocol|{
    let protocol_msg;
    match control_msg {
      ContactProtocol::ServerControl { state, command, cmd_args, time } => {
        protocol_msg = 
          Protocols::CPType(ContactProtocol::ServerControl { state: state, command: command, cmd_args: cmd_args, time: time });
      },
      ContactProtocol::ClientControl { state, command, cmd_args, time } => {
        protocol_msg = 
          Protocols::CPType(ContactProtocol::ClientControl { state: state, command: command, cmd_args: cmd_args, time: time });
      },
      ContactProtocol::User2UserMsg { state, target, content, time } => {
        protocol_msg = 
          Protocols::CPType(ContactProtocol::User2UserMsg { state: state, target: target, content: content, time: time });
      }
    };
    socket.send( &serde_json::to_vec(&protocol_msg).unwrap(),0).unwrap();
  };
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
        let quit_msg = ContactProtocol::ServerControl { state: MsgStatus::SUBMITTED, command: "shutdown".to_string(), cmd_args: None, time: Utc::now() };
        send_control(&control_socket, quit_msg);
        break;
      },
      "client" => {
        match cmd_it.next() {
          Some(val) => {
            match val {
              "list" => {
                let client_list_msg = 
                  ContactProtocol::ClientControl { state: MsgStatus::SUBMITTED, command: "get_clients".to_string(), cmd_args: None, time: Utc::now() };
                send_control(&control_socket, client_list_msg);
                let response_json_vec = control_socket.recv_bytes(0).unwrap();  
                let msg: Protocols = serde_json::from_slice(&response_json_vec).unwrap();
                if let Protocols::CPType(ContactProtocol::ClientControl { state, command, cmd_args, time }) = msg{
                  println!("clients: {}", cmd_args.unwrap());
                }
              },
              _ => {
                warn!("Invalid clint arg: {}", val);
                continue;
              }
            }
          },
          None => {
            warn!("client needs args");
            continue;
          }
        }
      }
      _ => {
        warn!("Unknow cmd");
        continue;
      }
    }
  }
  router_handle.join().unwrap();
  info!("Total exiting...");
}

