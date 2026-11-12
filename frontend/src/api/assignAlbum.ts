import { useMessageStore } from '@/store/messageStore'
import { useDataStore } from '@/store/dataStore'
import { IsolationId } from '@/type/types'
import axios from 'axios'
import { tryWithMessageStore } from '@/script/utils/try_catch'

export type AssignOutcome = 'moved' | 'renamedFrom' | 'skipped'

interface AssignAlbumResult {
  outcome: AssignOutcome
}

export async function assignAlbum(
  assetId: string,
  albumId: string,
  index: number,
  isolationId: IsolationId,
  onConflict: 'skip' | 'rename',
  opts: { notify?: boolean } = {}
): Promise<AssignOutcome | undefined> {
  const messageStore = useMessageStore('mainId')
  const dataStore = useDataStore(isolationId)
  const notify = opts.notify ?? true

  const success = await tryWithMessageStore('mainId', async () => {
    const body: {
      assetId: string
      albumId: string
      onConflict: 'skip' | 'rename'
    } = {
      assetId,
      albumId,
      onConflict
    }
    const response = await axios.put<AssignAlbumResult>('/put/assign_album', body)
    if (response.status !== 200) {
      throw new Error(`Server responded with status ${response.status}`)
    }
    dataStore.setAlbum(index, albumId)
    const outcome = response.data.outcome
    if (!notify) return outcome
    if (outcome === 'renamedFrom') {
      messageStore.success('Moved to album (file renamed).')
    } else if (outcome === 'skipped') {
      messageStore.info('File already exists in album; skipped.')
    } else {
      messageStore.success('Moved to album.')
    }
    return outcome
  })

  // tryWithMessageStore already yields undefined on failure.
  return success
}

/** Tally of a batch move, for the single summary toast. */
export interface AssignBatchTally {
  moved: number
  renamed: number
  skipped: number
  failed: number
}

/** One summary line for a batch move. Pure for testability. */
export function summarizeAssignOutcomes(tally: AssignBatchTally): string {
  const one = (n: number, word: string): string => `${n} ${word}${n === 1 ? '' : 's'}`
  const moved = tally.moved + tally.renamed
  const details: string[] = []
  if (tally.renamed > 0) details.push(`${one(tally.renamed, 'file')} renamed`)
  if (tally.skipped > 0) details.push(`${one(tally.skipped, 'file')} skipped`)
  if (tally.failed > 0) details.push(`${one(tally.failed, 'file')} failed`)
  if (moved === 0) return `No files moved${details.length > 0 ? ` (${details.join(', ')})` : ''}.`
  const head = `Moved ${one(moved, 'file')} to album`
  return details.length > 0 ? `${head} (${details.join(', ')}).` : `${head}.`
}

/**
 * Move several assets to one album with a single summary toast instead of
 * one toast per file. Per-file failures keep their own error toasts (via the
 * interceptor); the summary counts them. Continues past individual failures
 * so one bad file does not strand the rest of the selection.
 */
export async function assignAlbumBatch(
  items: { assetId: string; index: number }[],
  albumId: string,
  isolationId: IsolationId,
  onConflict: 'skip' | 'rename'
): Promise<void> {
  const messageStore = useMessageStore('mainId')
  const tally: AssignBatchTally = { moved: 0, renamed: 0, skipped: 0, failed: 0 }
  for (const item of items) {
    const outcome = await assignAlbum(item.assetId, albumId, item.index, isolationId, onConflict, {
      notify: false
    })
    if (outcome === 'renamedFrom') tally.renamed += 1
    else if (outcome === 'skipped') tally.skipped += 1
    else if (outcome === undefined) tally.failed += 1
    else tally.moved += 1
  }
  const total = tally.moved + tally.renamed + tally.skipped + tally.failed
  if (total === 0) return
  messageStore.success(summarizeAssignOutcomes(tally))
}
