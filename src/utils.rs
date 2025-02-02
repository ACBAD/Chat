use std::{fmt::write, io::Write};
use log::error;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

pub trait ZmqJsonServer {
  fn recv_json<Protocols: for<'a> Deserialize<'a>>(&self, flags: Option<i32>) -> Result<(String, Protocols), Box<dyn std::error::Error>>;
  fn send_json<Protocols: Serialize>(&self, client_id: &String, data: &Protocols, flags: Option<i32>) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait ZmqJsonClient {
  fn recv_json<Protocols: for<'a> Deserialize<'a>>(&self, flags: Option<i32>) -> Result<Protocols, Box<dyn std::error::Error>>; 
  fn send_json<Protocols: Serialize>(&self, data: &Protocols, flags: Option<i32>) -> Result<(), Box<dyn std::error::Error>>;
}

pub fn input(prompt: &str) -> String {
  print!("{}", prompt);
  std::io::stdout().flush().unwrap();
  let mut input = String::new();
  std::io::stdin().read_line(&mut input).unwrap_or_else(|e|{error!("error occured in reading stdin: {}", e.to_string());0});
  input.trim().to_string()
}

#[derive(Serialize, Deserialize, PartialEq)]
pub enum MsgStatus {
  SUBMITTED,
  ACCEPTED,
  FAILED,
  REJECTED,
}

impl std::fmt::Display for MsgStatus {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      MsgStatus::SUBMITTED => write!(f, "SUBMITTED"),
      MsgStatus::FAILED => write!(f, "FAILED"),
      MsgStatus::ACCEPTED => write!(f, "ACCEPTED"),
      MsgStatus::REJECTED => write!(f, "REJECTED"),
    }  
  }
}

#[derive(Serialize, Deserialize)]
pub enum ContactProtocol{
  ServerControl{state: MsgStatus, command: String, cmd_args: Option<serde_json::Value>, time: DateTime<Utc>},
  ClientControl{state: MsgStatus, command: String, cmd_args: Option<serde_json::Value>, time: DateTime<Utc>},
  User2UserMsg{state: MsgStatus, target: String, content: MessageType, time: DateTime<Utc>},
}

#[derive(Serialize, Deserialize)]
pub enum NotifyProtocol {
  MsgFromUser{sender: String, content: MessageType},
}

#[derive(Serialize, Deserialize)]
pub enum MessageType {
  TextMsg{content: String},
}

#[derive(Serialize, Deserialize)]
pub enum Protocols {
  CPType(ContactProtocol),
  NPType(NotifyProtocol),
}
