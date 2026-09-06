import { performance } from 'node:perf_hooks'
import { SessionTranscriptObserver as Original } from './original/lib/advisor/observer.js'
import { SessionTranscriptObserver as Optimized } from './lib/advisor/observer.js'
const mode = process.argv[2], historyTurns = Number(process.argv[3]), iterations = 100
const events = [], messages = []
let snapshots = 0, snapshotElements = 0, eventAt = 0, deltas = 0, chars = 0
const session = {
  id: 'bench', get seq() { return events.length },
  eventAt(seq) { eventAt++; return events[seq] },
  snapshotEvents() { snapshots++; snapshotElements += events.length; return events.slice() },
  deriveMessages() { return messages },
}
const observer = new (mode === 'original' ? Original : Optimized)({
  getSession: () => session,
  onDelta: (_id, delta) => { deltas++; chars += delta.charCount },
})
function append(type, data, notify) {
  const event = { type, data, seq: events.length, surfaceOp: 'append' }
  events.push(event)
  if (type === 'user/message') messages.push(data)
  if (type === 'assistant/message') messages.push(data.message)
  if (!notify) return
  if (mode === 'original') observer.handleEvent('bench', type === 'turn/end' ? session.snapshotEvents() : [], event)
  else observer.handleSessionEvent(session, event)
}
function turn(n, notify) {
  append('turn/start', { turn: n }, notify)
  append('user/message', { id: `user-${n}`, role: 'user', content: [{ type: 'text', text: 'synthetic user request' }], source: { kind: 'user' } }, notify)
  append('step/start', { turn: n, step: 0 }, notify)
  append('assistant/message', { message: { id: `assistant-${n}`, role: 'assistant', content: [{ type: 'text', text: 'synthetic assistant response' }] } }, notify)
  append('turn/end', { turn: n, reason: { kind: 'completed' } }, notify)
}
for (let n = 0; n < historyTurns; n++) turn(n, false)
observer.seedTo('bench', messages.length)
// Separate cold recovery cost from steady-state cost. Both implementations
// receive identical complete current history and identical subsequent events.
const coldStart = performance.now(); turn(historyTurns, true)
const coldMs = performance.now() - coldStart
const coldReads = { snapshots, snapshotElements, eventAt }
snapshots = 0; snapshotElements = 0; eventAt = 0; deltas = 0; chars = 0
global.gc()
const before = process.memoryUsage(); let peakHeap = before.heapUsed, peakRss = before.rss
const durations = []
for (let n = historyTurns + 1; n <= historyTurns + iterations; n++) {
  const start = performance.now(); turn(n, true); durations.push(performance.now() - start)
  const memory = process.memoryUsage()
  peakHeap = Math.max(peakHeap, memory.heapUsed); peakRss = Math.max(peakRss, memory.rss)
}
global.gc()
const after = process.memoryUsage(), sorted = [...durations].sort((a, b) => a - b)
console.log(JSON.stringify({ mode, historyTurns, iterations, coldMs, coldReads,
  meanMs: durations.reduce((a, b) => a + b, 0) / iterations,
  p50Ms: sorted[50], p95Ms: sorted[95], snapshots, snapshotElements, eventAt, deltas, chars,
  memory: { before, after, peakHeap, peakRss, maxRssKiB: process.resourceUsage().maxRSS } }))
