import { bcs, BcsType } from '@mysten/bcs'

export function lazy<T>(cb: () => BcsType<T>): BcsType<T> {
  let cached: BcsType<T> | null = null
  const get = (): BcsType<T> => {
    if (!cached) cached = cb()
    return cached
  }
  return new BcsType<T>({
    name: 'lazy' as never,
    read: (r) => get().read(r),
    write: (v, w) => get().write(v, w),
    serialize: (v, opts) => get().serialize(v, opts).toBytes(),
    serializedSize: (v) => get().serializedSize(v),
  })
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('')
}

export const Address = bcs.bytes(32)
export const U256 = bcs.bytes(32)

export const Sender = bcs.struct('Sender', {
  pk: U256,
  tweak: U256,
})

export const Table = bcs.struct('Table', {
  id: Address,
  size: bcs.u64(),
})

export const Feed = bcs.struct('Feed', {
  id: Address,
  counter: bcs.u64(),
})

export const Entity = bcs.struct('Entity', {
  feed: Feed,
  version: bcs.u32(),
})

export const EntityRoot = bcs.struct('EntityRoot', {
  id: Address,
  entity: Entity,
  genesis: bcs.bool(),
})

export const Bans = bcs.struct('Bans', {
  level: Address,
  ip32: bcs.u64(),
  ip24: bcs.u64(),
  ip20: bcs.u64(),
  ip16: bcs.u64(),
})

export const BanKey = bcs.struct('BanKey', {
  level: Address,
  mask: bcs.u8(),
  ip_hash: Address,
})

export const BanValue = bcs.struct('BanValue', {
  reason_hash: Address,
  expires: bcs.u64(),
})

export const Tripcode = bcs.struct('Tripcode', {
  secured: bcs.bool(),
  trip: bcs.byteVector(),
})

export const Responses = bcs.struct('Responses', {
  uid: bcs.option(bcs.byteVector()),
  ip32: bcs.option(Address),
  tripcode: bcs.option(Tripcode),
  geo: bcs.option(bcs.u32()),
})

export const Invocation = bcs.struct('Invocation', {
  program_hash: Address,
  function: bcs.string(),
  args: bcs.byteVector(),
})

export const IntentBody = bcs.struct('IntentBody', {
  version: bcs.u32(),
  root: Address,
  author: Address,
  tweak: Address,
  expected_seq: bcs.u64(),
  call: Invocation,
})

export const SignedIntent = bcs.struct('SignedIntent', {
  body: IntentBody,
  signature: bcs.bytes(64),
})
