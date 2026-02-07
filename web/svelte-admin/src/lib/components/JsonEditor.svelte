<script lang="ts">
  import type { ModelMappingEntry, ModelMappingValue } from '../types';

  export let id: string;
  export let label: string;
  export let value: Record<string, ModelMappingValue>;
  export let onChange: (next: Record<string, ModelMappingValue>) => void;
  export let onErrorChange: ((error: string | null) => void) | undefined =
    undefined;
  export let placeholder: string | undefined = undefined;
  export let helperText: string | undefined = undefined;
  export let rows = 6;

  let text = '';
  let error: string | null = null;
  let isEditing = false;

  function toPrettyJson(v: Record<string, ModelMappingValue>): string {
    return JSON.stringify(v ?? {}, null, 2);
  }

  function setError(next: string | null) {
    error = next;
    onErrorChange?.(next);
  }

  function tryParseModelMapping(
    raw: string
  ):
    | { ok: true; value: Record<string, ModelMappingValue> }
    | { ok: false; error: string } {
    const trimmed = raw.trim();
    if (!trimmed) return { ok: true, value: {} };

    let parsed: unknown;
    try {
      parsed = JSON.parse(trimmed);
    } catch {
      return { ok: false, error: 'Invalid JSON' };
    }

    if (
      typeof parsed !== 'object' ||
      parsed === null ||
      Array.isArray(parsed)
    ) {
      return { ok: false, error: 'JSON must be an object' };
    }

    const mapping: Record<string, ModelMappingValue> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === 'string') {
        // Simple format: "gpt-4": "gpt-4-turbo"
        mapping[k] = v;
      } else if (typeof v === 'object' && v !== null && !Array.isArray(v)) {
        // Extended format: check for mapped_model field
        const entry = v as Record<string, unknown>;
        if (typeof entry.mapped_model !== 'string') {
          return {
            ok: false,
            error: `Entry "${k}" must have a string "mapped_model" field`,
          };
        }
        mapping[k] = entry as unknown as ModelMappingEntry;
      } else {
        return {
          ok: false,
          error: `Value for "${k}" must be a string or an object with mapped_model`,
        };
      }
    }

    return { ok: true, value: mapping };
  }

  $: if (!isEditing) {
    text = toPrettyJson(value);
    setError(null);
  }

  function handleInput(nextText: string) {
    isEditing = true;
    text = nextText;

    const parsed = tryParseModelMapping(nextText);
    if (!parsed.ok) {
      setError(parsed.error);
      return;
    }

    setError(null);
    onChange(parsed.value);
  }

  function handleBlur() {
    isEditing = false;

    const parsed = tryParseModelMapping(text);
    if (!parsed.ok) return;
    text = toPrettyJson(parsed.value);
  }
</script>

<div>
  <label for={id} class="label">{label}</label>
  <textarea
    {id}
    class="input font-mono"
    {rows}
    {placeholder}
    value={text}
    aria-invalid={error ? 'true' : undefined}
    oninput={e => handleInput((e.currentTarget as HTMLTextAreaElement).value)}
    onblur={handleBlur}
  ></textarea>

  {#if helperText && !error}
    <p class="helper-text">{helperText}</p>
  {/if}
  {#if error}
    <p class="mt-1 text-xs text-red-600">{error}</p>
  {/if}
</div>
