import { derivePostKeypair, computeTweak, toHex } from './crypto'
import type { MessageKey } from '../../ui/i18n/ru'

export type RoleKind =
  | 'anonymous'
  | 'op'
  | 'forum_mod'
  | 'board_mod'
  | 'thread_mod'
  | 'forum_admin'
  | 'thread_admin'

export interface RoleOption {
  kind: RoleKind
  labelKey: MessageKey
  addressHex?: string
  tweakArgs?: (Uint8Array | string)[]
  rawTweak?: Uint8Array
  selectable?: boolean
}

export function matchesAuthor(master: Uint8Array, pk: Uint8Array, tweak: Uint8Array): boolean {
  const derived = derivePostKeypair(master, tweak)
  return toHex(derived.publicKey) === toHex(pk)
}

export function signable(role?: RoleOption | null): role is RoleOption {
  return !!role && (!!role.rawTweak || !!role.tweakArgs)
}

export function roleTweak(role: RoleOption): Uint8Array {
  if (role.rawTweak) return role.rawTweak
  if (role.tweakArgs) return computeTweak(...role.tweakArgs)
  throw new Error('role has no identity')
}

function roleCandidates(master: Uint8Array, scopeIdHex: string, tag: 'moder' | 'admin'): { pk: Uint8Array; tweakArgs: (Uint8Array | string)[] }[] {
  const idStr = scopeIdHex.replace(/^0x/, '')
  const tweak = computeTweak(idStr, tag)
  const { publicKey } = derivePostKeypair(master, tweak)
  return [{ pk: publicKey, tweakArgs: [idStr, tag] }]
}

function inList(hex: string, list: Uint8Array[]): boolean {
  return list.some(a => toHex(a) === hex)
}

function pushRole(
  roles: RoleOption[],
  kind: RoleKind,
  labelKey: MessageKey,
  list: Uint8Array[],
  cands: { pk: Uint8Array; tweakArgs: (Uint8Array | string)[] }[],
): void {
  for (const c of cands) {
    if (inList(toHex(c.pk), list)) {
      roles.push({ kind, labelKey, addressHex: toHex(c.pk), tweakArgs: c.tweakArgs })
      return
    }
  }
}

export interface Moderators {
  forum_admin: Uint8Array | null
  forum_mods: Uint8Array[]
  board_mods: Uint8Array[]
  thread_mods: Uint8Array[]
  thread_admin: Uint8Array | null
}

export interface RoleContext {
  master: Uint8Array
  forumIdHex: string
  boardUidHex?: string
  threadUidHex?: string
  moderators: Moderators
  op?: { tweak: Uint8Array; pk: Uint8Array } | null
}

export function threadAdminRole(
  master: Uint8Array,
  threadUidHex: string,
  admin: Uint8Array | null,
  op: { tweak: Uint8Array; pk: Uint8Array } | null,
): RoleOption | null {
  if (!admin) return null
  const candidate = roleCandidates(master, threadUidHex, 'admin')[0]
  if (toHex(candidate.pk) === toHex(admin)) {
    return { kind: 'thread_admin', labelKey: 'roles.threadAdmin', addressHex: toHex(candidate.pk), tweakArgs: candidate.tweakArgs }
  }
  if (op && toHex(op.pk) === toHex(admin)) {
    return {
      kind: 'thread_admin',
      labelKey: 'roles.threadAdmin',
      addressHex: toHex(op.pk),
      rawTweak: op.tweak,
      selectable: false,
    }
  }
  return null
}

export function withThreadRoles(viewRoles: RoleOption[], threadRoles: RoleOption[]): RoleOption[] {
  return [
    ...viewRoles.filter((role) => role.kind !== 'thread_admin' && role.kind !== 'thread_mod'),
    ...threadRoles,
  ]
}

export function detectRoles(ctx: RoleContext): RoleOption[] {
  const roles: RoleOption[] = [{ kind: 'anonymous', labelKey: 'roles.anonymous' }]
  const mods = ctx.moderators
  const adminList = (a: Uint8Array | null) => (a ? [a] : [])

  pushRole(roles, 'forum_mod', 'roles.forumMod', mods.forum_mods, roleCandidates(ctx.master, ctx.forumIdHex, 'moder'))

  if (ctx.boardUidHex) {
    pushRole(roles, 'board_mod', 'roles.boardMod', mods.board_mods, roleCandidates(ctx.master, ctx.boardUidHex, 'moder'))
  }

  if (ctx.threadUidHex) {
    pushRole(roles, 'thread_mod', 'roles.threadMod', mods.thread_mods, roleCandidates(ctx.master, ctx.threadUidHex, 'moder'))
    const adminRole = threadAdminRole(ctx.master, ctx.threadUidHex, mods.thread_admin, ctx.op ?? null)
    if (adminRole) roles.push(adminRole)
  }

  pushRole(roles, 'forum_admin', 'roles.forumAdmin', adminList(mods.forum_admin), roleCandidates(ctx.master, ctx.forumIdHex, 'admin'))

  if (ctx.op) {
    roles.push({
      kind: 'op',
      labelKey: 'roles.op',
      addressHex: toHex(ctx.op.pk),
      rawTweak: ctx.op.tweak,
    })
  }

  return roles
}

export type PostRole = 'op' | 'mop' | 'thread_mod' | 'thread_admin' | 'board_mod' | 'forum_mod' | 'forum_admin' | null

export function postRole(addr: Uint8Array, mods: Moderators, opAddressHex: string | null): PostRole {
  const hex = toHex(addr)
  if (opAddressHex !== null && hex === opAddressHex) {
    return mods.thread_admin && toHex(mods.thread_admin) === hex ? 'mop' : 'op'
  }
  return roleForAddress(addr, mods)
}

export function roleForAddress(addr: Uint8Array, mods: Moderators): PostRole {
  const hex = toHex(addr)
  if (mods.thread_admin && toHex(mods.thread_admin) === hex) return 'thread_admin'
  if (mods.forum_admin && toHex(mods.forum_admin) === hex) return 'forum_admin'
  const inList = (l: Uint8Array[]) => l.some(a => toHex(a) === hex)
  if (inList(mods.thread_mods)) return 'thread_mod'
  if (inList(mods.board_mods)) return 'board_mod'
  if (inList(mods.forum_mods)) return 'forum_mod'
  return null
}

export const ROLE_STYLE: Record<string, { text: string; color: string; bold: boolean }> = {
  op: { text: '#OP', color: '#008000', bold: false },
  mop: { text: '#MOP', color: '#ff6600', bold: false },
  thread_mod: { text: '# THREAD MOD #', color: '#ff6600', bold: false },
  thread_admin: { text: '# THREAD ADMIN #', color: '#cc0000', bold: false },
  board_mod: { text: '# BOARD MOD #', color: '#003366', bold: false },
  forum_mod: { text: '# MOD #', color: '#800080', bold: true },
  forum_admin: { text: '# ADMIN #', color: '#000000', bold: true },
}
