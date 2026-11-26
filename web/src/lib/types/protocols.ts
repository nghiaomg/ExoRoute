/** Protocols the dashboard accepts from clients. */
export type Protocol = 'chat_completions' | 'responses' | 'messages';

/** Protocols a provider upstream may speak, including Google. */
export type UpstreamProtocol = Protocol | 'google_generate_content';

/** Generic page/list response shape returned by dashboard endpoints. */
export type Collection<T> = T[] | { data: T[]; total?: number };
