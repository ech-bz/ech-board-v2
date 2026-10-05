import { bcs } from '@mysten/bcs'
import { Address, BanKey, BanValue } from '../bcs/common'
import { call, type CallDesc } from './core'

export interface BanKeyInput {
  level: Uint8Array
  mask: number
  ipHash: Uint8Array
}

export interface BanValueInput {
  reasonHash: Uint8Array
  expires: number | bigint
}

const keyValue = bcs.tuple([BanKey, BanValue])
const keyOnly = bcs.tuple([BanKey])
const boardPair = bcs.tuple([Address, BanKey, BanValue])
const boardKey = bcs.tuple([Address, BanKey])
const threadPair = bcs.tuple([Address, Address, BanKey, BanValue])
const threadKey = bcs.tuple([Address, Address, BanKey])
const postPair = bcs.tuple([Address, Address, Address, BanKey, BanValue])
const postKey = bcs.tuple([Address, Address, Address, BanKey])

function keyOf(key: BanKeyInput) {
  return { level: key.level, mask: key.mask, ip_hash: key.ipHash }
}

function valueOf(value: BanValueInput) {
  return { reason_hash: value.reasonHash, expires: value.expires }
}

export const BanEvents = {
  forumBan: (key: BanKeyInput, value: BanValueInput): CallDesc =>
    call('forum_ban', () => keyValue.serialize([keyOf(key), valueOf(value)]).toBytes()),
  forumUnban: (key: BanKeyInput): CallDesc =>
    call('forum_unban', () => keyOnly.serialize([keyOf(key)]).toBytes()),
  boardBan: (key: BanKeyInput, value: BanValueInput): CallDesc =>
    call('board_ban', (ids) => boardPair.serialize([ids.board!, keyOf(key), valueOf(value)]).toBytes()),
  boardUnban: (key: BanKeyInput): CallDesc =>
    call('board_unban', (ids) => boardKey.serialize([ids.board!, keyOf(key)]).toBytes()),
  threadBan: (key: BanKeyInput, value: BanValueInput): CallDesc =>
    call('thread_ban', (ids) => threadPair.serialize([ids.board!, ids.thread!, keyOf(key), valueOf(value)]).toBytes()),
  threadUnban: (key: BanKeyInput): CallDesc =>
    call('thread_unban', (ids) => threadKey.serialize([ids.board!, ids.thread!, keyOf(key)]).toBytes()),
  forumBanPost: (key: BanKeyInput, value: BanValueInput): CallDesc =>
    call('forum_ban_post', (ids) => postPair.serialize([ids.board!, ids.thread!, ids.post!, keyOf(key), valueOf(value)]).toBytes()),
  forumUnbanPost: (key: BanKeyInput): CallDesc =>
    call('forum_unban_post', (ids) => postKey.serialize([ids.board!, ids.thread!, ids.post!, keyOf(key)]).toBytes()),
  boardBanPost: (key: BanKeyInput, value: BanValueInput): CallDesc =>
    call('board_ban_post', (ids) => postPair.serialize([ids.board!, ids.thread!, ids.post!, keyOf(key), valueOf(value)]).toBytes()),
  boardUnbanPost: (key: BanKeyInput): CallDesc =>
    call('board_unban_post', (ids) => postKey.serialize([ids.board!, ids.thread!, ids.post!, keyOf(key)]).toBytes()),
  threadBanPost: (key: BanKeyInput, value: BanValueInput): CallDesc =>
    call('thread_ban_post', (ids) => postPair.serialize([ids.board!, ids.thread!, ids.post!, keyOf(key), valueOf(value)]).toBytes()),
  threadUnbanPost: (key: BanKeyInput): CallDesc =>
    call('thread_unban_post', (ids) => postKey.serialize([ids.board!, ids.thread!, ids.post!, keyOf(key)]).toBytes()),
}
