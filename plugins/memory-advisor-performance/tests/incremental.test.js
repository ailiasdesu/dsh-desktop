import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SessionTranscriptObserver as Original } from '../original/lib/advisor/observer.js'
import { SessionTranscriptObserver as Optimized } from '../lib/advisor/observer.js'
import { Session } from 'file:///C:/Users/34021/AppData/Local/DSH%20Desktop/kernel/node_modules/@deepseek-ai/dsh-session/lib/index.js'

export function rig(Class, modern = true, session = null) {
  const calls = [], counts = { snapshots: 0, eventAt: 0 }
  let events = [], messages = []
  let current = session ?? {
    id: 'test', get seq() { return events.length },
    eventAt(seq) { return events[seq] },
    snapshotEvents() { return events.slice() },
    deriveMessages() { return messages },
  }
  const instrument = () => {
    const snapshot = current.snapshotEvents.bind(current), at = current.eventAt.bind(current)
    current.snapshotEvents = (...args) => { counts.snapshots++; return snapshot(...args) }
    current.eventAt = (...args) => { counts.eventAt++; return at(...args) }
  }
  instrument()
  const observer = new Class({
    getSession: () => current,
    onDelta: (...args) => calls.push(['delta', ...args]),
    onSteppedTurnEnd: (...args) => calls.push(['end', ...args]),
    onRewrite: (...args) => calls.push(['rewrite', ...args]),
  })
  const deliver = event => {
    if (modern && observer.handleSessionEvent) observer.handleSessionEvent(current, event)
    else observer.handleEvent(current.id, event.type === 'turn/end' ? current.snapshotEvents() : [], event)
  }
  return {
    observer, calls, counts, deliver,
    push(type, data = {}, notify = true, surfaceOp = 'append') {
      let event
      if (session) event = current.append(type, data, ...(['user/message', 'assistant/message'].includes(type) ? [{ surfaceOp }] : []))
      else {
        event = { type, data, seq: events.length, surfaceOp }
        events.push(event)
        if (type === 'user/message') messages.push(data)
        if (type === 'assistant/message') messages.push(data.message)
      }
      if (notify) deliver(event)
      return event
    },
    replacement() {
      events = []; messages = []
      current = { id: 'test', get seq() { return events.length }, eventAt: seq => events[seq], snapshotEvents: () => events.slice(), deriveMessages: () => messages }
      instrument()
    },
    current: () => current,
  }
}
export function turn(r, n, { notify = true, step = true, reason = 'completed', compact = false } = {}) {
  r.push('turn/start', { turn: n }, notify)
  r.push('user/message', { id: `u${n}`, role: 'user', content: [{ type: 'text', text: 'hello' }], source: { kind: 'user' } }, notify)
  if (step) r.push('step/start', { turn: n, step: 1 }, notify)
  r.push('assistant/message', { message: { id: `a${n}`, role: 'assistant', content: [{ type: 'text', text: 'world' }], source: { kind: 'model', provider: 'fixture', model: 'fixture' } } }, notify)
  if (compact) r.push('compact/end', {}, notify)
  return r.push('turn/end', { turn: n, reason: { kind: reason } }, notify)
}

for (const scenario of ['complete', 'hot-subscribe', 'gap', 'duplicate', 'same-id-new-session', 'agentic', 'legacy', 'broken-eventAt']) {
  test(`original/optimized callback equivalence: ${scenario}`, () => {
    const old = rig(Original), next = rig(Optimized, scenario !== 'legacy')
    for (const r of [old, next]) {
      if (scenario === 'hot-subscribe') {
        turn(r, 0, { notify: false })
        r.push('step/start', { turn: 1 }, false)
        r.push('turn/end', { turn: 1, reason: { kind: 'completed' } })
      }
      if (scenario === 'gap') {
        r.push('step/start', { turn: 99 }, false)
        r.push('turn/end', { turn: 99, reason: { kind: 'error' } })
      }
      if (scenario === 'broken-eventAt') {
        r.push('step/start', { turn: 98 }, false)
        r.current().eventAt = () => undefined
        r.push('turn/end', { turn: 98, reason: { kind: 'completed' } })
      }
      if (scenario === 'agentic') {
        r.push('assistant/message', { message: { id: 'a', role: 'assistant', content: [{ type: 'text', text: 'answer' }] } })
        r.push('user/message', { id: 'u', role: 'user', content: [{ type: 'text', text: 'next' }], source: { kind: 'user' } })
      }
      for (let n = 2; n < 12; n++) {
        const event = turn(r, n, { step: n !== 3, reason: ['completed', 'max-tokens', 'error', 'aborted', 'blocked', 'interrupted'][n % 6], compact: n === 6 })
        if (scenario === 'duplicate') r.deliver(event)
      }
      if (scenario === 'same-id-new-session') {
        r.replacement()
        turn(r, 13, { step: false })
        turn(r, 14)
        r.observer.disposeSession('test'); r.replacement(); turn(r, 15)
      }
    }
    assert.deepEqual(next.calls, old.calls)
    if (scenario === 'complete') assert.deepEqual(next.counts, { snapshots: 0, eventAt: 0 })
    if (scenario === 'hot-subscribe' || scenario === 'gap') {
      assert.equal(next.counts.snapshots, 0)
      assert.ok(next.counts.eventAt > 0)
    }
    if (scenario === 'broken-eventAt') assert.ok(next.counts.snapshots > 0)
  })
}

test('real official Session append: equivalent callbacks without full snapshot', () => {
  const old = rig(Original, true, Session.create('test'))
  const next = rig(Optimized, true, Session.create('test'))
  for (const r of [old, next]) { turn(r, 0); turn(r, 1, { reason: 'aborted' }); turn(r, 2) }
  assert.deepEqual(next.calls, old.calls)
  assert.equal(next.counts.snapshots, 0)
})

test('real official Session seeded resume: missing prefix recovered once with eventAt', () => {
  const source = rig(Original, true, Session.create('test'))
  turn(source, 0)
  source.push('turn/start', { turn: 1 })
  source.push('step/start', { turn: 1, step: 0 })
  const seed = source.current().snapshotEvents()
  const old = rig(Original, true, Session.create('test', seed))
  const next = rig(Optimized, true, Session.create('test', seed))
  for (const r of [old, next]) {
    r.push('turn/end', { turn: 1, reason: { kind: 'error' } })
    turn(r, 2)
  }
  assert.deepEqual(next.calls, old.calls)
  assert.equal(next.counts.snapshots, 0)
  assert.ok(next.counts.eventAt > 0)
  const reads = next.counts.eventAt
  turn(next, 3)
  assert.equal(next.counts.eventAt, reads)
})

test('synchronous newer log end and skipped publication reproduce full-snapshot gate', () => {
  const old = rig(Original), next = rig(Optimized)
  for (const r of [old, next]) {
    const first = turn(r, 0, { notify: false })
    turn(r, 1, { notify: false })
    r.deliver(first)
    turn(r, 2)
  }
  assert.deepEqual(next.calls, old.calls)
  assert.equal(next.counts.snapshots, 0)
})
