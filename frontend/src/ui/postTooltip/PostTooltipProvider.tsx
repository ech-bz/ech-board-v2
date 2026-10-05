import { createSignal, onCleanup, Show, For, type JSX } from 'solid-js'
import { Portal } from 'solid-js/web'
import { Ctx, type Api, type PostTooltipTarget, type ResolvePost } from './usePostTooltip'
import { TooltipLayer, type Entry } from './TooltipLayer'
import type { TooltipPost } from '../../core/load'

let nextId = 1
const LEAVE_DELAY = 500

export function PostTooltipProvider(props: {
  resolve: ResolvePost
  boardPage?: boolean
  children: JSX.Element
}) {
  const [entries, setEntries] = createSignal<Entry[]>([])
  const [fading, setFading] = createSignal<ReadonlySet<number>>(new Set())
  let current: Entry[] = []
  let closeTimer: number | null = null
  let lastMove: PointerEvent | null = null
  const elements = new Map<number, HTMLElement>()
  const fadeTimers = new Set<number>()

  const update = (next: Entry[]) => {
    current = next
    const live = new Set(next.map((entry) => entry.id))
    setEntries(entries().filter((entry) => !live.has(entry.id)).concat(next))
  }

  const clearClose = () => {
    if (closeTimer != null) {
      window.clearTimeout(closeTimer)
      closeTimer = null
    }
  }

  const fade = (removed: Entry[]) => {
    if (removed.length === 0) return
    const el = elements.get(removed[0].id)
    const ms = el ? parseFloat(getComputedStyle(el).transitionDuration) * 1000 : 0
    const ids = removed.map((entry) => entry.id)
    setFading((prev) => new Set([...prev, ...ids]))
    const timer = window.setTimeout(() => {
      fadeTimers.delete(timer)
      setEntries((list) => list.filter((entry) => !ids.includes(entry.id)))
      setFading((prev) => {
        const next = new Set(prev)
        for (const id of ids) next.delete(id)
        return next
      })
    }, ms)
    fadeTimers.add(timer)
  }

  const closeAll = () => {
    clearClose()
    fade(current)
    elements.clear()
    update([])
  }

  const open = (anchor: HTMLElement, target: PostTooltipTarget) => {
    if (current.some((entry) => entry.anchor === anchor)) return
    clearClose()
    let nested = false
    for (let i = current.length - 1; i >= 0; i--) {
      const el = elements.get(current[i].id)
      if (el && el.contains(anchor)) {
        nested = true
        break
      }
    }
    const [result, setResult] = createSignal<TooltipPost | null>(null)
    const [error, setError] = createSignal(false)
    const entry: Entry = { id: nextId++, anchor, target, result, error }
    if (nested) {
      update([...current, entry])
    } else {
      fade(current)
      update([entry])
    }
    props
      .resolve(target.postUid)
      .then((value) => {
        setResult(value)
        setError(!value)
      })
      .catch(() => setError(true))
  }

  const isOpen = (anchor: HTMLElement) => current.some((entry) => entry.anchor === anchor)
  const api: Api = { open, isOpen }

  const isOver = (el: HTMLElement | null | undefined, e: PointerEvent): boolean => {
    if (!el) return false
    if (el.contains(e.target as Node)) return true
    const r = el.getBoundingClientRect()
    const pad = 8
    return e.clientX >= r.left - pad && e.clientX <= r.right + pad && e.clientY >= r.top - pad && e.clientY <= r.bottom + pad
  }

  const deepestUnder = (e: PointerEvent): number => {
    let deeperKept = false
    let deepestKept = -1
    for (let i = current.length - 1; i >= 0; i--) {
      const inOwn = isOver(elements.get(current[i].id), e) || isOver(current[i].anchor, e)
      const stay: boolean = inOwn || deeperKept
      if (stay && deepestKept === -1) deepestKept = i
      deeperKept = stay
    }
    return deepestKept
  }

  const truncate = (keep: number) => {
    if (keep === 0) {
      closeAll()
      return
    }
    if (keep >= current.length) return
    const removed = current.slice(keep)
    fade(removed)
    for (const entry of removed) elements.delete(entry.id)
    update(current.slice(0, keep))
  }

  const keepUnder = (e: PointerEvent, delayed: boolean): boolean => {
    if (current.length === 0) {
      clearClose()
      return false
    }
    const deepestKept = deepestUnder(e)
    const keep = deepestKept === -1 ? 0 : deepestKept + 1
    if (keep === current.length) {
      clearClose()
      return true
    }
    if (!delayed) {
      truncate(keep)
      return keep > 0
    }
    if (closeTimer == null) {
      closeTimer = window.setTimeout(() => {
        closeTimer = null
        const deepest = lastMove ? deepestUnder(lastMove) : -1
        truncate(deepest === -1 ? 0 : deepest + 1)
      }, LEAVE_DELAY)
    }
    return true
  }

  const onMove = (e: PointerEvent) => {
    lastMove = e
    keepUnder(e, true)
  }
  const onDown = (e: PointerEvent) => {
    lastMove = e
    keepUnder(e, false)
  }

  document.addEventListener('pointermove', onMove, true)
  document.addEventListener('pointerdown', onDown, true)
  onCleanup(() => {
    clearClose()
    for (const timer of fadeTimers) window.clearTimeout(timer)
    document.removeEventListener('pointermove', onMove, true)
    document.removeEventListener('pointerdown', onDown, true)
  })

  return (
    <Ctx.Provider value={api}>
      {props.children}
      <Show when={entries().length > 0}>
        <Portal>
          <For each={entries()}>
            {(entry) => (
              <TooltipLayer
                entry={entry}
                boardPage={props.boardPage}
                closing={() => fading().has(entry.id)}
                register={(el) => {
                  if (fading().has(entry.id)) return
                  if (el) elements.set(entry.id, el)
                  else elements.delete(entry.id)
                }}
              />
            )}
          </For>
        </Portal>
      </Show>
    </Ctx.Provider>
  )
}
