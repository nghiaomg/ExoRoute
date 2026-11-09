let nextMessageId = 0;

export function nextId(): string {
  nextMessageId += 1;
  return `chat-msg-${nextMessageId}`;
}
