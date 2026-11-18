use crate::message::{AssistantTurn, Message, ToolSpec};
use anyhow::Result;
use std::sync::Arc;

/// Backend that can run one chat turn. Implemented by oc-provider.
pub trait Chat: Send + Sync {
    fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<AssistantTurn>;

    /// Streaming variant; default falls back to non-streaming.
    fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        _on_delta: &mut dyn FnMut(&str),
    ) -> Result<AssistantTurn> {
        self.chat(messages, tools)
    }
}

impl<T: Chat + ?Sized> Chat for Box<T> {
    fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<AssistantTurn> {
        (**self).chat(messages, tools)
    }
    fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<AssistantTurn> {
        (**self).chat_stream(messages, tools, on_delta)
    }
}

impl<T: Chat + ?Sized> Chat for Arc<T> {
    fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<AssistantTurn> {
        (**self).chat(messages, tools)
    }
    fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<AssistantTurn> {
        (**self).chat_stream(messages, tools, on_delta)
    }
}
