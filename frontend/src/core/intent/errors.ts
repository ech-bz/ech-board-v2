export interface MoveAbortInfo {
  module: string
  code: number
  message: string
}

const RELAY_MESSAGES: [RegExp, string][] = [
  [/banned/, 'Этот адрес заблокирован'],
  [/captcha/, 'Капча не пройдена'],
  [/unauthorized/, 'Нет прав'],
  [/not found/, 'Не найдено'],
  [/invalid request/, 'Некорректный запрос'],
  [/no intents provided/, 'Интент не передан'],
  [/intent root mismatch/, 'Несовпадение форума интента'],
  [/unknown program/, 'Неизвестная программа'],
  [/intent signature invalid/, 'Неверная подпись интента'],
  [/unsupported function/, 'Неподдерживаемая функция'],
  [/media count .* exceeds/, 'Превышен лимит вложений'],
  [/content hash mismatch/, 'Несовпадение хеша вложения'],
  [/text content mismatch/, 'Несовпадение хеша текста'],
]

export function parseMoveAbort(e: unknown): MoveAbortInfo | null {
  const raw = e instanceof Error ? e.message : String(e)
  let text = raw
  try {
    const parsed = JSON.parse(raw)
    if (parsed && typeof parsed.error === 'string') text = parsed.error
  } catch {}
  for (const [pattern, message] of RELAY_MESSAGES) {
    if (pattern.test(text)) return { module: 'relay', code: 0, message }
  }
  return { module: 'relay', code: 0, message: text }
}
