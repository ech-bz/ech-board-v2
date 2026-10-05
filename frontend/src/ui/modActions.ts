import { createSignal } from 'solid-js'
import { blake2b } from '@noble/hashes/blake2'
import { bcs } from '@mysten/bcs'
import { Address } from '../core/bcs/common'
import { createIntentBuilder } from '../core/intent/IntentBuilder'
import { PostEvent } from '../core/events/post_apply'
import { ThreadEvent } from '../core/events/thread_apply'
import { ForumEvent } from '../core/events/forum_apply'
import { BoardEvent } from '../core/events/board_apply'
import { BanEvents } from '../core/events/ban_apply'
import { call, type CallDesc } from '../core/events/core'
import { PostProjection, serializePostParts } from '../core/bcs/types'
import type { PostObject, ThreadObject, BoardObject, PostPartInput } from '../core/bcs/types'
import {
  roleForPostAction,
  roleForThreadAction,
  roleForBan,
  roleForForumSettings,
  roleForBoardCore,
  roleForBoardSoft,
  roleForThreadAdminMgmt,
  SELF_MOD_WINDOW_MS,
} from '../core/intent/capabilities'
import type { BanScope } from '../core/intent/capabilities'
import { fromHex, toHex, derivePostKeypair, sign, signRaw } from '../core/intent/crypto'
import { parseBbCode, finalizeParts } from '../core/intent/bbcode'
import { makePostResolver } from '../core/intent/resolver'
import { matchPostsByAuthor } from '../core/intent/author'
import { sendIntents, fetchBanHashes, fetchDecrypt, fetchNonce } from '../core/api/relay'
import { getSecretKeyBytes } from '../core/keys/storage'
import type { RoleOption } from '../core/intent/roles'
import { roleTweak, signable } from '../core/intent/roles'
import type { EntityNs } from '../core/intent/upgrade'

export interface ModEnv {
  forumId: string
  rootKey: string
  programHash: string
  relayUrl: string
  boardUid: string
  slug?: string
  roles: RoleOption[]
  rolesForThread: (threadUid: string) => RoleOption[]
  myTweaks: Map<string, Uint8Array>
  postByUid: (uidHex: string) => string | null
  onDone?: () => Promise<void> | void
  onPostDone?: (post: PostObject) => Promise<void> | void
  onPostContent?: (hashHex: string, blob: Uint8Array) => void
  onError: (e: unknown) => void
}

type PostModAction = 'delete' | 'restore'
type ThreadModAction = 'close' | 'open' | 'delete' | 'restore'

const BATCH_CHUNK = 100

const maskIndex = (mask: number): number => (mask === 32 ? 0 : mask === 24 ? 1 : mask === 20 ? 2 : 3)

function banScopeVars(scope: BanScope, forumId: string, boardUid: string, threadUid: string, durationMs: number, reason: string) {
  const pathHex = scope === 'forum'
    ? [forumId]
    : scope === 'board'
      ? [forumId, boardUid]
      : [forumId, boardUid, threadUid]
  const reasonHash = blake2b(new TextEncoder().encode(reason), { dkLen: 32 })
  const expires = durationMs >= Number.MAX_SAFE_INTEGER ? BigInt('18446744073709551615') : Date.now() + durationMs
  const level = fromHex(scope === 'forum' ? forumId : scope === 'board' ? boardUid : threadUid)
  return { pathHex, reasonHash, expires, level }
}

function concatBytes(...arrays: Uint8Array[]): Uint8Array {
  const total = arrays.reduce((n, a) => n + a.length, 0)
  const out = new Uint8Array(total)
  let off = 0
  for (const a of arrays) {
    out.set(a, off)
    off += a.length
  }
  return out
}

const entityIdArg = bcs.tuple([Address])

export function createModActions(getEnv: () => ModEnv) {
  const [busy, setBusy] = createSignal(false)

  const runAction = async (fn: () => Promise<void>, done?: () => Promise<void> | void) => {
    if (busy()) return
    const env = getEnv()
    setBusy(true)
    try {
      await fn()
      await (done ?? env.onDone)?.()
    } catch (e) {
      env.onError(e)
    }
    setBusy(false)
  }

  const signPost = (post: PostObject, env: ModEnv) => {
    const pp = new PostProjection(post.projection)
    const myTweak = env.myTweaks.get(toHex(pp.sender().pk))
    const ageMs = Date.now() - Number(pp.timestamp_ms())
    if (myTweak && ageMs <= SELF_MOD_WINDOW_MS) return { rawTweak: myTweak } as RoleOption
    const role = roleForPostAction('edit', env.roles)
    if (!signable(role)) throw new Error('no role for post action')
    return role
  }

  const sendRoleIntent = async (env: ModEnv, desc: CallDesc, role: RoleOption | null, ids: { board?: string; thread?: string; post?: Uint8Array }, items?: { text?: Uint8Array; topic?: string; description?: string }, reason?: string) => {
    if (!signable(role)) throw new Error('no role for action')
    let builder = createIntentBuilder(env)
      .event(desc)
      .board(fromHex(ids.board ?? env.boardUid))
    if (ids.thread) builder = builder.thread(fromHex(ids.thread))
    if (ids.post) builder = builder.post(ids.post)
    const intent = await builder.role(role).build()
    await sendIntents(env.relayUrl, [{ intent, ...items }], { reason })
  }

  const runPostDelete = (post: PostObject, action: PostModAction, threadUid: string) =>
    runAction(async () => {
      const env = getEnv()
      await sendRoleIntent(
        env,
        PostEvent.setDeleted(action === 'delete'),
        signPost(post, env),
        { thread: threadUid, post: post.root.id },
      )
    })

  const runPostEdit = (
    post: PostObject,
    threadUid: string,
    text: string,
    postMap: Map<number, { id: Uint8Array; author: Uint8Array }>,
    secretRefs?: Map<number, PostPartInput>,
  ) =>
    runAction(
      async () => {
        const env = getEnv()
        const builder = createIntentBuilder(env)
          .board(fromHex(env.boardUid))
          .thread(fromHex(threadUid))
          .post(post.root.id)
          .role(signPost(post, env))
        const { secretKey, publicKey } = builder.deriveKeypair()
        const parsed = await parseBbCode(text, makePostResolver({ forumId: env.forumId, boardUid: env.boardUid, slug: env.slug, relayUrl: env.relayUrl, postMap }))
        const parts = await finalizeParts(parsed, secretKey, publicKey, secretRefs)
        const textBlob = parts.length > 0 ? serializePostParts(parts) : null
        const textHash = textBlob ? blake2b(textBlob, { dkLen: 32 }) : null
        const intent = await builder.event(PostEvent.setText(textHash)).build()
        await sendIntents(env.relayUrl, [{ intent, text: textBlob ?? undefined }])
        if (textBlob && textHash) env.onPostContent?.(toHex(textHash), textBlob)
      },
      () => getEnv().onPostDone?.(post),
    )

  const runPostBanMedia = (post: PostObject, threadUid: string, hashes: Uint8Array[]) =>
    runAction(
      async () => {
        const env = getEnv()
        await sendRoleIntent(
          env,
          PostEvent.banMedia(hashes.map((h) => new Uint8Array(h))),
          signPost(post, env),
          { thread: threadUid, post: post.root.id },
        )
      },
      () => getEnv().onPostDone?.(post),
    )

  const runPostUnbanMedia = (post: PostObject, threadUid: string, hashes: Uint8Array[]) =>
    runAction(
      async () => {
        const env = getEnv()
        const role = roleForPostAction('unban_media', env.roles)
        await sendRoleIntent(
          env,
          PostEvent.unbanMedia(hashes.map((h) => new Uint8Array(h))),
          role,
          { thread: threadUid, post: post.root.id },
        )
      },
      () => getEnv().onPostDone?.(post),
    )

  const banRoleSigner = (env: ModEnv, scope: BanScope) => {
    const role = roleForBan(scope, env.roles)
    if (!signable(role)) throw new Error(`no role for ${scope} ban`)
    const { secretKey, publicKey } = derivePostKeypair(getSecretKeyBytes(), roleTweak(role))
    return { secretKey, publicKey }
  }

  const postBanKeyValue = async (env: ModEnv, scope: BanScope, mask: number, postUidHex: string, vars: ReturnType<typeof banScopeVars>) => {
    const { secretKey, publicKey } = banRoleSigner(env, scope)
    const signature = await signRaw(secretKey, fromHex(postUidHex))
    const hashes = await fetchBanHashes(env.relayUrl, postUidHex, { pk: publicKey, signature })
    return {
      key: { level: vars.level, mask, ipHash: hashes[maskIndex(mask)] },
      value: { reasonHash: vars.reasonHash, expires: vars.expires },
    }
  }

  const uidBanKeyValue = async (env: ModEnv, scope: BanScope, mask: number, uidHex: string, vars: ReturnType<typeof banScopeVars>) => {
    const { secretKey, publicKey } = banRoleSigner(env, scope)
    const uid = fromHex(uidHex)
    const pathHex = [...vars.pathHex]
    if (scope === 'thread') {
      const postHex = env.postByUid(uidHex)
      if (!postHex) throw new Error('uid not found in this thread')
      pathHex.push(postHex)
    }
    const message = concatBytes(uid, ...pathHex.map(fromHex), publicKey)
    const signature = await sign(secretKey, message)
    const hashes = await fetchDecrypt(env.relayUrl, { uid, path: pathHex, pk: publicKey, signature })
    return {
      key: { level: vars.level, mask, ipHash: hashes[maskIndex(mask)] },
      value: { reasonHash: vars.reasonHash, expires: vars.expires },
    }
  }

  const banDesc = (scope: BanScope, build: () => Promise<{ key: { level: Uint8Array; mask: number; ipHash: Uint8Array }; value: { reasonHash: Uint8Array; expires: number | bigint } }>, post: boolean): (() => Promise<CallDesc>) => async () => {
    const { key, value } = await build()
    if (scope === 'forum') return post ? BanEvents.forumBanPost(key, value) : BanEvents.forumBan(key, value)
    if (scope === 'board') return post ? BanEvents.boardBanPost(key, value) : BanEvents.boardBan(key, value)
    return post ? BanEvents.threadBanPost(key, value) : BanEvents.threadBan(key, value)
  }

  const runBan = (post: PostObject, threadUid: string, scope: BanScope, mask: number, durationMs: number, reason: string) =>
    runAction(
      async () => {
        const env = getEnv()
        const vars = banScopeVars(scope, env.forumId, env.boardUid, threadUid, durationMs, reason)
        const role = roleForBan(scope, env.roles)
        const desc = await banDesc(scope, () => postBanKeyValue(env, scope, mask, toHex(post.root.id), vars), true)()
        await sendRoleIntent(env, desc, role, { thread: threadUid, post: post.root.id }, undefined, reason)
      },
      () => getEnv().onPostDone?.(post),
    )

  const runBanUid = (uidHex: string, threadUid: string, scope: BanScope, mask: number, durationMs: number, reason: string) =>
    runAction(async () => {
      const env = getEnv()
      const vars = banScopeVars(scope, env.forumId, env.boardUid, threadUid, durationMs, reason)
      const role = roleForBan(scope, env.roles)
      const desc = await banDesc(scope, () => uidBanKeyValue(env, scope, mask, uidHex, vars), false)()
      await sendRoleIntent(env, desc, role, { thread: scope === 'thread' ? threadUid : undefined }, undefined, reason)
    })

  const runBatchPostDelete = async (posts: PostObject[], threadUid: string, onProgress?: (done: number, total: number) => void) => {
    const env = getEnv()
    const targets = posts.filter((p) => !new PostProjection(p.projection).deleted())
    const role = roleForPostAction('delete', env.roles)
    if (!signable(role)) throw new Error('no role for batch delete')
    const sender = createIntentBuilder(env)
      .board(fromHex(env.boardUid))
      .thread(fromHex(threadUid))
      .role(role)
      .deriveKeypair().publicKey
    let done = 0
    for (let start = 0; start < targets.length; start += BATCH_CHUNK) {
      const chunk = targets.slice(start, start + BATCH_CHUNK)
      const base = await fetchNonce(env.relayUrl, sender)
      const items: { intent: Uint8Array }[] = []
      for (let i = 0; i < chunk.length; i++) {
        const intent = await createIntentBuilder(env)
          .event(PostEvent.setDeleted(true))
          .board(fromHex(env.boardUid))
          .thread(fromHex(threadUid))
          .post(chunk[i].root.id)
          .role(role)
          .build(base + i)
        items.push({ intent })
        done++
        onProgress?.(done, targets.length)
      }
      await sendIntents(env.relayUrl, items)
    }
  }

  const runBatchPostRestore = async (posts: PostObject[], threadUid: string, onProgress?: (done: number, total: number) => void) => {
    const env = getEnv()
    const targets = posts.filter((p) => new PostProjection(p.projection).deleted())
    const role = roleForPostAction('restore', env.roles)
    if (!signable(role)) throw new Error('no role for batch restore')
    const sender = createIntentBuilder(env)
      .board(fromHex(env.boardUid))
      .thread(fromHex(threadUid))
      .role(role)
      .deriveKeypair().publicKey
    let done = 0
    for (let start = 0; start < targets.length; start += BATCH_CHUNK) {
      const chunk = targets.slice(start, start + BATCH_CHUNK)
      const base = await fetchNonce(env.relayUrl, sender)
      const items: { intent: Uint8Array }[] = []
      for (let i = 0; i < chunk.length; i++) {
        const intent = await createIntentBuilder(env)
          .event(PostEvent.setDeleted(false))
          .board(fromHex(env.boardUid))
          .thread(fromHex(threadUid))
          .post(chunk[i].root.id)
          .role(role)
          .build(base + i)
        items.push({ intent })
        done++
        onProgress?.(done, targets.length)
      }
      await sendIntents(env.relayUrl, items)
    }
  }

  const runBatchBan = async (
    posts: PostObject[],
    threadUid: string,
    scope: BanScope,
    mask: number,
    durationMs: number,
    reason: string,
    onProgress?: (done: number, total: number) => void,
  ) => {
    const env = getEnv()
    const role = roleForBan(scope, env.roles)
    if (!signable(role)) throw new Error(`no role for ${scope} ban`)
    const sender = createIntentBuilder(env)
      .board(fromHex(env.boardUid))
      .thread(fromHex(threadUid))
      .role(role)
      .deriveKeypair().publicKey
    let done = 0
    for (let start = 0; start < posts.length; start += BATCH_CHUNK) {
      const chunk = posts.slice(start, start + BATCH_CHUNK)
      const base = await fetchNonce(env.relayUrl, sender)
      const items: { intent: Uint8Array }[] = []
      for (let i = 0; i < chunk.length; i++) {
        const post = chunk[i]
        const vars = banScopeVars(scope, env.forumId, env.boardUid, threadUid, durationMs, reason)
        const desc = await banDesc(scope, () => postBanKeyValue(env, scope, mask, toHex(post.root.id), vars), true)()
        const intent = await createIntentBuilder(env)
          .event(desc)
          .board(fromHex(env.boardUid))
          .thread(fromHex(threadUid))
          .post(post.root.id)
          .role(role)
          .build(base + i)
        items.push({ intent })
        done++
        onProgress?.(done, posts.length)
      }
      await sendIntents(env.relayUrl, items, { reason })
    }
  }

  const runUnban = (post: PostObject, threadUid: string, scope: BanScope) =>
    runAction(
      async () => {
        const env = getEnv()
        const banned = new PostProjection(post.projection).banned()
        if (!banned) throw new Error('post is not banned')
        const key = { level: banned.level, mask: banned.mask, ipHash: banned.ip_hash }
        const desc =
          scope === 'forum'
            ? BanEvents.forumUnban(key)
            : scope === 'board'
              ? BanEvents.boardUnban(key)
              : BanEvents.threadUnban(key)
        const role = roleForBan(scope, env.roles)
        await sendRoleIntent(env, desc, role, { thread: threadUid, post: post.root.id })
      },
      () => getEnv().onPostDone?.(post),
    )

  const sendThreadEvent = async (action: ThreadModAction, threadUid: string) => {
    const env = getEnv()
    const desc =
      action === 'close'
        ? ThreadEvent.setClosed(true)
        : action === 'open'
          ? ThreadEvent.setClosed(false)
          : action === 'delete'
            ? ThreadEvent.setDeleted(true)
            : ThreadEvent.setDeleted(false)
    const role = roleForThreadAction(action, env.rolesForThread(threadUid))
    await sendRoleIntent(env, desc, role, { thread: threadUid })
  }

  const runThreadAction = (action: ThreadModAction, threadUid: string) => runAction(() => sendThreadEvent(action, threadUid))

  const runThreadCloseDelete = (threadUid: string) =>
    runAction(async () => {
      const env = getEnv()
      const role = roleForThreadAction('delete', env.rolesForThread(threadUid))
      if (!signable(role)) throw new Error('no role for close/delete')
      const sender = createIntentBuilder(env)
        .board(fromHex(env.boardUid))
        .thread(fromHex(threadUid))
        .role(role)
        .deriveKeypair().publicKey
      const base = await fetchNonce(env.relayUrl, sender)
      const items: { intent: Uint8Array }[] = []
      const descs = [ThreadEvent.setClosed(true), ThreadEvent.setDeleted(true)]
      for (const [i, desc] of descs.entries()) {
        const intent = await createIntentBuilder(env)
          .event(desc)
          .board(fromHex(env.boardUid))
          .thread(fromHex(threadUid))
          .role(role)
          .build(base + i)
        items.push({ intent })
      }
      await sendIntents(env.relayUrl, items)
    })

  const runThreadEditTopic = (threadUid: string, topic: string) =>
    runAction(async () => {
      const env = getEnv()
      const topicText = topic.trim()
      const topicHash = topicText ? blake2b(new TextEncoder().encode(topicText), { dkLen: 32 }) : null
      const role = roleForThreadAction('edit_topic', env.rolesForThread(threadUid))
      await sendRoleIntent(env, ThreadEvent.setTopic(topicHash), role, { thread: threadUid }, { topic: topicText || undefined })
      if (topicText && topicHash) env.onPostContent?.(toHex(topicHash), new TextEncoder().encode(topicText))
    })

  const upgradeCall = (ns: EntityNs, object: PostObject | ThreadObject | BoardObject | null): CallDesc => {
    if (ns === 'forum') return call('upgrade_forum', () => new Uint8Array(0))
    if (!object) throw new Error('upgrade target missing')
    if (ns === 'board') return call('upgrade_board', () => entityIdArg.serialize([object.root.id]).toBytes())
    if (ns === 'thread') return call('upgrade_thread', () => entityIdArg.serialize([object.root.id]).toBytes())
    return call('upgrade_post', () => entityIdArg.serialize([object.root.id]).toBytes())
  }

  const runUpgrade = (ns: EntityNs, object: PostObject | ThreadObject | BoardObject | null) =>
    runAction(
      async () => {
        const env = getEnv()
        const desc = upgradeCall(ns, object)
        let builder = createIntentBuilder(env)
          .event(desc)
          .board(fromHex(env.boardUid))
        if (ns === 'thread' || ns === 'post') {
          const threadUid = ns === 'post' ? new PostProjection((object as PostObject).projection).thread() : object!.root.id
          builder = builder.thread(threadUid)
        }
        if (ns === 'post') builder = builder.post(object!.root.id)
        const intent = await builder.build()
        await sendIntents(env.relayUrl, [{ intent }])
      },
      () => {
        if (ns !== 'post' || !object) return
        getEnv().onPostDone?.(object as PostObject)
      },
    )

  const runIntent = (
    desc: CallDesc,
    role: RoleOption | null,
    threadUid: string | null,
    extras?: { textBlob?: Uint8Array; descriptionText?: string; topicText?: string },
    boardUidArg?: string,
  ) =>
    runAction(async () => {
      const env = getEnv()
      await sendRoleIntent(
        env,
        desc,
        role,
        { board: boardUidArg ?? env.boardUid, thread: threadUid ?? undefined },
        { text: extras?.textBlob, description: extras?.descriptionText, topic: extras?.topicText },
      )
    })

  const runForumSetTimestampPrecision = (precision: number) =>
    runIntent(ForumEvent.setTimestampPrecision(precision), roleForForumSettings(getEnv().roles), null)

  const runBoardSetDescription = (text: string) => {
    const hash = text ? blake2b(new TextEncoder().encode(text), { dkLen: 32 }) : null
    return runIntent(BoardEvent.setDescription(hash), roleForBoardSoft(getEnv().roles), null, { descriptionText: text })
  }

  const runBoardSetMaxMedia = (v: number | bigint) => runIntent(BoardEvent.setMaxMedia(v), roleForBoardCore(getEnv().roles), null)

  const runBoardSetBumpLimit = (v: number | bigint) => runIntent(BoardEvent.setBumpLimit(v), roleForBoardCore(getEnv().roles), null)

  const runBoardSetIgnoreForumBans = (v: boolean) => runIntent(BoardEvent.setIgnoreForumBans(v), roleForBoardCore(getEnv().roles), null)

  const runBoardSetReactions = (vals: Uint8Array[]) => runIntent(BoardEvent.setReactions(vals), roleForBoardSoft(getEnv().roles), null)

  const runBoardSetPinned = (pinned: Uint8Array[], boardUidArg?: string) =>
    runIntent(BoardEvent.setPinned(pinned), roleForBoardSoft(getEnv().roles), null, undefined, boardUidArg)

  const runBoardAddModerator = (pk: Uint8Array) => runIntent(BoardEvent.addModerator(pk), roleForBoardCore(getEnv().roles), null)

  const runBoardDelModerator = (pk: Uint8Array) => runIntent(BoardEvent.delModerator(pk), roleForBoardCore(getEnv().roles), null)

  const runBoardClose = (boardUidArg: string, v: boolean) => runIntent(BoardEvent.setClosed(v), roleForBoardCore(getEnv().roles), null, undefined, boardUidArg)

  const runBoardDelete = (boardUidArg: string, v: boolean) => runIntent(BoardEvent.setDeleted(v), roleForBoardCore(getEnv().roles), null, undefined, boardUidArg)

  const runThreadSetAdmin = (threadUid: string, pk: Uint8Array | null) =>
    runIntent(ThreadEvent.setAdmin(pk), roleForThreadAdminMgmt(getEnv().rolesForThread(threadUid)), threadUid)

  const runThreadAddModerator = (threadUid: string, pk: Uint8Array) =>
    runIntent(ThreadEvent.addModerator(pk), roleForThreadAdminMgmt(getEnv().rolesForThread(threadUid)), threadUid)

  const runThreadDelModerator = (threadUid: string, pk: Uint8Array) =>
    runIntent(ThreadEvent.delModerator(pk), roleForThreadAdminMgmt(getEnv().rolesForThread(threadUid)), threadUid)

  const findAuthorPosts = (ref: PostObject, posts: PostObject[], threadUid: string, onProgress?: (done: number, total: number) => void): Promise<string[]> => {
    const env = getEnv()
    const role = roleForBan('thread', env.roles)
    if (!signable(role)) throw new Error('no role for author match')
    const { secretKey, publicKey } = derivePostKeypair(getSecretKeyBytes(), roleTweak(role))
    return matchPostsByAuthor({
      relayUrl: env.relayUrl,
      pathHex: [env.forumId, env.boardUid, threadUid],
      pk: publicKey,
      secretKey,
      refId: toHex(ref.root.id),
      candidates: posts.map((p) => ({ id: toHex(p.root.id), uid: new PostProjection(p.projection).uid() })),
      onProgress,
    })
  }

  return {
    busy,
    runPostDelete,
    runPostEdit,
    runPostBanMedia,
    runPostUnbanMedia,
    runThreadAction,
    runThreadCloseDelete,
    runThreadEditTopic,
    runUpgrade,
    runBan,
    runBanUid,
    runUnban,
    runBatchPostDelete,
    runBatchPostRestore,
    runBatchBan,
    runForumSetTimestampPrecision,
    runBoardSetDescription,
    runBoardSetMaxMedia,
    runBoardSetBumpLimit,
    runBoardSetIgnoreForumBans,
    runBoardSetReactions,
    runBoardSetPinned,
    runBoardAddModerator,
    runBoardDelModerator,
    runBoardClose,
    runBoardDelete,
    runThreadSetAdmin,
    runThreadAddModerator,
    runThreadDelModerator,
    findAuthorPosts,
  }
}

export type ModActions = ReturnType<typeof createModActions>
