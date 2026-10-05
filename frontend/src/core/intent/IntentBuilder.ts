import { computeTweak, derivePostKeypair, fromHex, signRaw } from './crypto'
import { fetchNonce } from '../api/relay'
import { IntentBody, SignedIntent } from '../bcs/common'
import type { CallDesc, CallIds } from '../events/core'
import type { RoleOption } from './roles'
import { getSecretKeyBytes } from '../keys/storage'

export interface IntentCtx {
  rootKey: string
  forumId: string
  programHash: string
  relayUrl: string
  masterSecret?: Uint8Array
}

const INTENT_PREFIX = new TextEncoder().encode('intent')

export class IntentBuilder {
  private _call: CallDesc | null = null
  private _boardId?: Uint8Array
  private _threadId?: Uint8Array
  private _postId?: Uint8Array
  private _tweakArgs?: (Uint8Array | string)[]
  private _tweak?: Uint8Array
  private _seq?: number

  constructor(
    private ctx: IntentCtx,
    private masterSecret: Uint8Array,
  ) {}

  event(desc: CallDesc): this {
    this._call = desc
    return this
  }

  board(id: Uint8Array): this {
    this._boardId = id
    return this
  }

  thread(id: Uint8Array): this {
    this._threadId = id
    return this
  }

  post(id: Uint8Array): this {
    this._postId = id
    return this
  }

  tweak(...args: (Uint8Array | string)[]): this {
    this._tweakArgs = args
    return this
  }

  rawTweak(bytes: Uint8Array): this {
    this._tweak = bytes
    return this
  }

  role(opt?: RoleOption | null): this {
    if (opt?.rawTweak) return this.rawTweak(opt.rawTweak)
    if (opt?.tweakArgs) return this.tweak(...opt.tweakArgs)
    return this
  }

  seq(seq: number): this {
    this._seq = seq
    return this
  }

  deriveKeypair(): { secretKey: Uint8Array; publicKey: Uint8Array; tweak: Uint8Array } {
    const tweak = this._tweak
      ?? (this._tweakArgs
        ? computeTweak(...this._tweakArgs)
        : crypto.getRandomValues(new Uint8Array(32)))
    this._tweak = tweak
    const { secretKey, publicKey } = derivePostKeypair(this.masterSecret, tweak)
    return { secretKey, publicKey, tweak }
  }

  async build(nonceOverride?: number): Promise<Uint8Array> {
    const desc = this._call
    if (!desc) throw new Error('event not set')
    const { secretKey, publicKey, tweak } = this.deriveKeypair()
    const ids: CallIds = { board: this._boardId, thread: this._threadId, post: this._postId }
    const args = desc.args(ids)
    const expected = nonceOverride ?? this._seq ?? await fetchNonce(this.ctx.relayUrl, publicKey)
    const body = {
      version: 1,
      root: fromHex(this.ctx.rootKey),
      author: publicKey,
      tweak,
      expected_seq: BigInt(expected),
      call: {
        program_hash: fromHex(this.ctx.programHash),
        function: desc.fn,
        args,
      },
    }
    const encoded = IntentBody.serialize(body).toBytes()
    const signingBytes = new Uint8Array(INTENT_PREFIX.length + encoded.length)
    signingBytes.set(INTENT_PREFIX, 0)
    signingBytes.set(encoded, INTENT_PREFIX.length)
    const signature = await signRaw(secretKey, signingBytes)
    return SignedIntent.serialize({ body, signature }).toBytes()
  }
}

export function createIntentBuilder(ctx: IntentCtx): IntentBuilder {
  return new IntentBuilder(ctx, ctx.masterSecret ?? getSecretKeyBytes())
}
