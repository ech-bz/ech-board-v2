use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum PostPart {
    Plain(String),
    Bold(Vec<PostPart>),
    Italic(Vec<PostPart>),
    Code(Vec<PostPart>),
    ReplyTo {
        forum: [u8; 32],
        board: [u8; 32],
        post: [u8; 32],
        hint: String,
    },
    Secret {
        data_nonce: [u8; 12],
        data_ct: Vec<u8>,
        encrypted_keys: Vec<EncryptedKey>,
    },
    Underline(Vec<PostPart>),
    Overline(Vec<PostPart>),
    Spoiler(Vec<PostPart>),
    Strike(Vec<PostPart>),
    Sup(Vec<PostPart>),
    Sub(Vec<PostPart>),
    Link {
        url: String,
        children: Vec<PostPart>,
    },
    Fold {
        title: String,
        children: Vec<PostPart>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EncryptedKey {
    pub nonce: [u8; 12],
    pub ct: [u8; 32],
    pub tag: [u8; 16],
}
