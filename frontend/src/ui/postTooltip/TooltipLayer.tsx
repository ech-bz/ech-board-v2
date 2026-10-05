import { createMemo, createSignal, onCleanup, Show } from 'solid-js'
import { cva } from 'class-variance-authority'
import { ThreadProjection } from '../../core/bcs/types'
import { PostBody } from '../components/Post'
import { previewRecord } from './previewRecord'
import { useEntityActions } from '../entityActions'
import { useStore } from '../store'
import type { PostTooltipTarget } from './usePostTooltip'
import type { TooltipPost } from '../../core/load'
import type { Accessor } from 'solid-js'

export interface Entry {
  id: number
  anchor: HTMLElement
  target: PostTooltipTarget
  result: Accessor<TooltipPost | null>
  error: Accessor<boolean>
}

const tooltipLayerVariants = cva('absolute z-tooltip w-max transition-opacity duration-[150ms]', {
  variants: {
    hidden: {
      true: 'invisible',
    },
    shown: {
      true: 'opacity-100',
      false: 'opacity-0',
    },
    closing: {
      true: 'pointer-events-none',
    },
  },
})
const tooltipLoadingVariants = cva('h-6 w-tooltip')
const TOOLTIP_OFFSET_X = 24

export function TooltipLayer(props: {
  entry: Entry
  register: (el: HTMLElement | null) => void
  boardPage?: boolean
  closing?: Accessor<boolean>
}) {
  const actions = useEntityActions()
  const store = useStore()
  let root: HTMLDivElement | undefined
  const [pos, setPos] = createSignal<{ left: number; top: number } | null>(null)
  const [reactionsRev, setReactionsRev] = createSignal(0)
  const [shown, setShown] = createSignal(false)
  const enter = requestAnimationFrame(() => requestAnimationFrame(() => setShown(true)))

  onCleanup(() => cancelAnimationFrame(enter))
  onCleanup(store.subscribeReactions(() => setReactionsRev((value) => value + 1)))

  const edges = (el: HTMLElement) => {
    const content = el.firstElementChild instanceof HTMLElement ? el.firstElementChild : el
    const tr = content.getBoundingClientRect()
    let bottom = tr.bottom
    let right = tr.right
    for (const over of el.querySelectorAll<HTMLElement>('[data-overhang]')) {
      const r = over.getBoundingClientRect()
      bottom = Math.max(bottom, r.bottom)
      right = Math.max(right, r.right)
    }
    return { tr, bottom, right }
  }

  const wall = () => {
    const el = document.querySelector<HTMLElement>('[data-post-column]') ?? document.querySelector('main')
    if (!el) return { left: 0, right: window.innerWidth }
    const r = el.getBoundingClientRect()
    const cs = getComputedStyle(el)
    return { left: r.left + parseFloat(cs.paddingLeft), right: r.right - parseFloat(cs.paddingRight) }
  }

  const measure = () => {
    if (!root) return
    const el = root
    const ar = props.entry.anchor.getBoundingClientRect()
    const { left: wallLeft, right: wallRight } = wall()
    const { tr, bottom, right } = edges(el)
    const width = tr.width
    const overhang = Math.max(0, right - tr.right)
    const maxWidthCss = `min(var(--post-max-w), ${Math.max(0, wallRight - wallLeft)}px)`
    if (el.style.maxWidth !== maxWidthCss) el.style.maxWidth = maxWidthCss
    const outerHeight = bottom - tr.top
    const header = [...document.querySelectorAll<HTMLElement>('[data-page-header]')].find((h) => h.offsetHeight > 0)
    const topLimit = header ? header.getBoundingClientRect().bottom + 8 : 8
    let bottomLimit = window.innerHeight - 8
    for (const b of document.querySelectorAll<HTMLElement>('[data-composer], [data-tabs]')) {
      if (b.offsetHeight === 0) continue
      const r = b.getBoundingClientRect()
      if (r.top > topLimit && r.top < bottomLimit) bottomLimit = r.top
    }
    const top = ar.bottom + outerHeight > bottomLimit ? ar.top - outerHeight : ar.bottom
    const left = Math.max(wallLeft, Math.min(ar.left + TOOLTIP_OFFSET_X, wallRight - width - overhang))
    setPos({ left: left + window.scrollX, top: top + window.scrollY })
  }

  window.addEventListener('resize', measure)
  const observer = new ResizeObserver(measure)
  onCleanup(() => {
    window.removeEventListener('resize', measure)
    observer.disconnect()
  })

  const record = createMemo(() => {
    reactionsRev()
    const res = props.entry.result()
    if (!res) return null
    return previewRecord(res, actions.roleKinds(), store.pendingReactionOf(props.entry.target.postUid), store.ordinalOf(props.entry.target.postUid))
  })

  const slug = () => props.entry.result()?.slug
  const threadNum = () => {
    const thread = props.entry.result()?.threadObj
    return thread ? Number(new ThreadProjection(thread.projection).number()) : undefined
  }
  const closed = () => {
    const thread = props.entry.result()?.threadObj
    return thread ? new ThreadProjection(thread.projection).closed() : false
  }
  const broom = () => {
    const rec = record()
    return !!rec && rec.op && rec.role === 'mop'
  }

  return (
    <div
      ref={(el) => {
        root = el
        props.register(el)
        if (el) {
          observer.observe(el)
          const parent = props.entry.anchor.closest<HTMLElement>('[data-tooltip-layer]')
          if (parent) observer.observe(parent)
        }
        measure()
      }}
      data-tooltip-layer=""
      class={tooltipLayerVariants({ hidden: !pos(), shown: shown() && !props.closing?.(), closing: props.closing?.() })}
      style={pos() ? { left: `${pos()!.left}px`, top: `${pos()!.top}px` } : undefined}
    >
      <Show when={record()} fallback={<div class={tooltipLoadingVariants()} />}>
        {(rec) => (
          <PostBody
            record={rec()}
            ctx={{
              decryptCtx: props.entry.result()!.ctx.decryptCtx,
              relayUrl: props.entry.result()!.ctx.relayUrl,
              forumId: props.entry.result()!.ctx.forumId,
              rootKey: props.entry.result()!.ctx.rootKey,
              boardUid: props.entry.result()!.ctx.boardUid,
            }}
            threadClosed={closed()}
            threadBroom={broom()}
            slug={slug()}
            threadNum={threadNum()}
            boardPage={props.boardPage}
            tooltip
          />
        )}
      </Show>
    </div>
  )
}
