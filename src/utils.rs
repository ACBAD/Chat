use std::io::{Write};
use log::{error};
use zmq;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

pub trait ZmqJsonCommunication {
  fn send_json<T: Serialize>(&self, data: &T, flags: Option<i32>) -> Result<(), Box<dyn std::error::Error>>;
  fn recv_json<T: for <'a>Deserialize<'a>>(&self, flags: Option<i32>) -> Result<T, Box<dyn std::error::Error>>;
}

impl ZmqJsonCommunication for zmq::Socket {
  fn send_json<T: Serialize>(&self, data: &T, flags: Option<i32>) -> Result<(), Box<dyn std::error::Error>> {
    let json_bytes = serde_json::to_vec(data)?;
    self.send(&json_bytes, flags.unwrap_or(0))?;
    Ok(())
  }
  fn recv_json<T: for <'a>Deserialize<'a>>(&self, flags: Option<i32>) -> Result<T, Box<dyn std::error::Error>> {
    let bytes = self.recv_bytes(flags.unwrap_or(0))?;
    let data = serde_json::from_slice(&bytes)?;
    Ok(data)
  }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ChatMsg {
  pub state: i8,
  pub msg_type: i8,
  pub error_msg: String,
  pub time: DateTime<Utc>,
  pub msg: String,
}

pub fn input(prompt: &str) -> String {
  print!("{}", prompt);
  std::io::stdout().flush().unwrap();
  let mut input = String::new();
  std::io::stdin().read_line(&mut input).unwrap_or_else(|e|{error!("error occured in reading stdin: {}", e.to_string());0});
  input.trim().to_string()
}


