/** Incremental equivalent of findLastMessageTurnEnd, keyed by Session identity.
 * Only public contiguous-sequence/eventAt APIs are used. Missing prefixes (hot
 * install, resume markers or missed publications) are recovered on demand.
 */
export class SteppedTurnTracker {
  states = new WeakMap()

  accept(session, event, needsLatest) {
    if (typeof session?.eventAt !== 'function' ||
        !Number.isSafeInteger(session.seq) || session.seq < 0 ||
        !Number.isSafeInteger(event?.seq) || event.seq < 0 || event.seq >= session.seq) {
      if (session && typeof session === 'object') this.states.delete(session)
      return { supported: false }
    }
    let state = this.states.get(session)
    if (!state || state.next > session.seq) {
      state = { next: 0, stepped: new Set(), latest: undefined }
      this.states.set(session, state)
    }
    const fold = (item) => {
      if (item.type === 'step/start') state.stepped.add(item.data?.turn)
      if (item.type === 'turn/end' && state.stepped.delete(item.data?.turn)) state.latest = item
      state.next++
    }
    // A gap is recovered below, never folded out of order. Ordinary complete
    // publications need no log reads and retain only unfinished stepped turns.
    if (event.seq === state.next) fold(event)
    if (needsLatest) {
      try {
        // Read through the current log end, matching the original full snapshot
        // even when an earlier listener synchronously appended another event.
        const end = session.seq
        while (state.next < end) {
          const item = session.eventAt(state.next)
          if (!item || item.seq !== state.next) throw new Error('non-contiguous session log')
          fold(item)
        }
      } catch {
        this.states.delete(session)
        return { supported: false }
      }
    }
    return { supported: true, latest: state.latest }
  }
}
