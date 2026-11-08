/**
 * Gallery row height in pixels.
 *
 * `scale` is the user's thumbnail-size setting and IS the row height: on a
 * wide window the result is exactly `scale`. The `windowWidth / 2` term is a
 * narrow-screen guard only — it binds when `windowWidth < 2 * scale`, where
 * rows shrink to half the window width instead of overflowing.
 *
 * Shared by the layout worker (worker/toDataWorker.ts) and the grey placeholder
 * (components/Buffer/BufferPlaceholder.vue) so both sides compute identical
 * geometry from the same inputs.
 */
export function rowBaseHeight(windowWidth: number, scale: number): number {
  return Math.round(Math.min(windowWidth / 2, scale))
}
