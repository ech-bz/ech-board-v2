import { onCleanup, type JSX } from 'solid-js'
import { usePostTooltip, type PostTooltipTarget } from './usePostTooltip'

const OPEN_DELAY = 50

export function PostRefLink(props: {
  label: JSX.Element
  class?: string
  href?: string
  target: PostTooltipTarget
  onClick?: (e: MouseEvent) => void
}) {
  const { open, isOpen } = usePostTooltip()
  const coarse = window.matchMedia('(pointer: coarse)').matches
  let timer: number | null = null
  const cancel = () => {
    if (timer != null) {
      window.clearTimeout(timer)
      timer = null
    }
  }
  onCleanup(cancel)
  return (
    <a
      class={props.class}
      href={props.href}
      onClick={(e) => {
        if (coarse) {
          if (!isOpen(e.currentTarget)) {
            e.preventDefault()
            open(e.currentTarget, props.target)
            return
          }
        }
        props.onClick?.(e)
      }}
      onMouseEnter={(e) => {
        if (coarse) return
        cancel()
        const el = e.currentTarget
        timer = window.setTimeout(() => {
          timer = null
          open(el, props.target)
        }, OPEN_DELAY)
      }}
      onMouseLeave={cancel}
    >
      {props.label}
    </a>
  )
}
