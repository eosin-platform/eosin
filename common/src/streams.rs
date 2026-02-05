use crate::types::Party;
use bytes::Bytes;
use uuid::Uuid;

pub const WAD_ANALYSIS: &str = "DORCH_WAD_ANALYSIS";
pub const MAP_ANALYSIS: &str = "DORCH_MAP_ANALYSIS";
pub const IMAGES: &str = "DORCH_IMAGES";

pub enum LeaveReason {
    Left,
    Kicked,
}

impl TryFrom<&u8> for LeaveReason {
    type Error = anyhow::Error;

    fn try_from(value: &u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(LeaveReason::Left),
            1 => Ok(LeaveReason::Kicked),
            _ => Err(anyhow::anyhow!("Invalid LeaveReason value: {}", value)),
        }
    }
}

impl From<LeaveReason> for u8 {
    fn from(message_type: LeaveReason) -> Self {
        match message_type {
            LeaveReason::Left => 0,
            LeaveReason::Kicked => 1,
        }
    }
}

pub enum WebsockMessageType {
    Message,
    Typing,
    StopTyping,
    MemberJoined,
    MemberLeft,
    PartyInfo,
    Invite,
    GameInfo,
}

impl WebsockMessageType {
    pub fn game_info(value: &[u8]) -> Bytes {
        let mut payload = Vec::with_capacity(value.len() + 1);
        payload.push(WebsockMessageType::GameInfo.into());
        payload.extend(value);
        payload.into()
    }

    pub fn invite(party_id: Uuid, sender_id: Uuid) -> Bytes {
        let mut payload = Vec::with_capacity(33);
        payload.push(WebsockMessageType::Invite.into());
        payload.extend_from_slice(party_id.as_bytes());
        payload.extend_from_slice(sender_id.as_bytes());
        payload.into()
    }

    pub fn member_joined(party_id: Uuid, user_id: Uuid) -> Bytes {
        let mut payload = Vec::with_capacity(33);
        payload.push(WebsockMessageType::MemberJoined.into());
        payload.extend(party_id.as_bytes());
        payload.extend(user_id.as_bytes());
        payload.into()
    }

    pub fn member_left(party_id: Uuid, user_id: Uuid, reason: LeaveReason) -> Bytes {
        let mut payload = Vec::with_capacity(33);
        payload.push(WebsockMessageType::MemberLeft.into());
        payload.extend(party_id.as_bytes());
        payload.extend(user_id.as_bytes());
        payload.push(reason.into());
        payload.into()
    }

    pub fn party_info(party: &Party) -> Bytes {
        let Party {
            id,
            name,
            leader_id,
            members,
        } = &party;
        let mut payload = Vec::with_capacity(
            33 + name.as_ref().map(|n| n.len()).unwrap_or_default()
                + 2
                + members
                    .as_ref()
                    .map(|m| 2 + m.len() * 16)
                    .unwrap_or_default(),
        );
        payload.push(WebsockMessageType::PartyInfo.into()); // 1
        payload.extend(id.as_bytes()); // 16
        payload.extend(leader_id.as_bytes()); // 16
        if let Some(name) = name {
            let name = name.as_bytes();
            payload.extend(&(name.len() as u16).to_le_bytes()); // name.len() as u16
            payload.extend(&name[..name.len().min(u16::MAX as usize)]);
        } else {
            payload.extend(&0u16.to_le_bytes()); // zero length
        }
        if let Some(members) = members {
            payload.extend(&(members.len() as u16).to_le_bytes());
            for member in members[..members.len().min(u16::MAX as usize)].iter() {
                payload.extend(member.as_bytes());
            }
        } // don't put anything if None
        payload.into()
    }
}

impl TryFrom<&u8> for WebsockMessageType {
    type Error = anyhow::Error;

    fn try_from(value: &u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(WebsockMessageType::Message),
            1 => Ok(WebsockMessageType::Typing),
            2 => Ok(WebsockMessageType::StopTyping),
            3 => Ok(WebsockMessageType::MemberJoined),
            4 => Ok(WebsockMessageType::MemberLeft),
            5 => Ok(WebsockMessageType::PartyInfo),
            6 => Ok(WebsockMessageType::Invite),
            7 => Ok(WebsockMessageType::GameInfo),
            _ => Err(anyhow::anyhow!(
                "Invalid WebsockMessageType value: {}",
                value
            )),
        }
    }
}

impl From<WebsockMessageType> for u8 {
    fn from(message_type: WebsockMessageType) -> Self {
        match message_type {
            WebsockMessageType::Message => 0,
            WebsockMessageType::Typing => 1,
            WebsockMessageType::StopTyping => 2,
            WebsockMessageType::MemberJoined => 3,
            WebsockMessageType::MemberLeft => 4,
            WebsockMessageType::PartyInfo => 5,
            WebsockMessageType::Invite => 6,
            WebsockMessageType::GameInfo => 7,
        }
    }
}

pub mod subjects {
    use std::fmt::Display;

    pub const MASTER: &str = "dorch.master";

    pub fn user<T>(user_id: T) -> String
    where
        T: Display,
    {
        format!("dorch.user.{}", user_id)
    }

    pub fn party<T>(thread_id: T) -> String
    where
        T: Display,
    {
        format!("dorch.party.{}", thread_id)
    }

    pub fn images<T>(wad_id: T) -> String
    where
        T: Display,
    {
        format!("dorch.wad.{}.img", wad_id)
    }

    pub mod analysis {
        use super::*;

        pub fn wad<T>(wad_id: T) -> String
        where
            T: Display,
        {
            format!("dorch.analysis.wad.{}", wad_id)
        }

        pub fn map<T, U>(wad_id: T, map: U) -> String
        where
            T: Display,
            U: Display,
        {
            format!("dorch.analysis.map.{}.{}", wad_id, map)
        }
    }
}
