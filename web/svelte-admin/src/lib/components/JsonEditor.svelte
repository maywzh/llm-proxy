<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    highlightSpecialChars,
  } from '@codemirror/view';
  import { EditorState } from '@codemirror/state';
  import { bracketMatching, indentOnInput } from '@codemirror/language';
  import { json } from '@codemirror/lang-json';
  import {
    defaultKeymap,
    indentWithTab,
    history,
    historyKeymap,
  } from '@codemirror/commands';
  import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
  import { highlightSelectionMatches } from '@codemirror/search';
  import { Maximize2, Minimize2 } from 'lucide-svelte';
  import type { ModelMappingEntry, ModelMappingValue } from '../types';

  interface Props {
    id: string;
    label: string;
    value: Record<string, ModelMappingValue>;
    onChange: (next: Record<string, ModelMappingValue>) => void;
    onErrorChange?: (error: string | null) => void;
    rows?: number;
    placeholder?: string;
    helperText?: string;
  }

  let {
    id,
    label,
    value,
    onChange,
    onErrorChange,
    rows = 6,
    placeholder: _placeholder,
    helperText,
  }: Props = $props();

  let error = $state<string | null>(null);
  let maximized = $state(false);
  let editorContainer: HTMLDivElement;
  let fullscreenEditorContainer: HTMLDivElement;
  let view: EditorView | null = null;
  let isInternalUpdate = false;

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
        mapping[k] = v;
      } else if (typeof v === 'object' && v !== null && !Array.isArray(v)) {
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

  const darkTheme = EditorView.theme({
    '&': {
      backgroundColor: '#1e1e2e',
      color: '#cdd6f4',
      fontSize: '13px',
    },
    '.cm-content': {
      caretColor: '#f5e0dc',
      fontFamily:
        'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace',
    },
    '.cm-gutters': {
      backgroundColor: '#181825',
      color: '#6c7086',
      border: 'none',
    },
    '.cm-activeLine': {
      backgroundColor: '#313244',
    },
    '.cm-activeLineGutter': {
      backgroundColor: '#313244',
    },
    '&.cm-focused .cm-cursor': {
      borderLeftColor: '#f5e0dc',
    },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
      backgroundColor: '#45475a',
    },
  });

  function formatIfValid() {
    if (!view) return;
    const text = view.state.doc.toString();
    const parsed = tryParseModelMapping(text);
    if (!parsed.ok) return;
    const pretty = toPrettyJson(parsed.value);
    if (pretty !== text) {
      isInternalUpdate = true;
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: pretty },
      });
      isInternalUpdate = false;
    }
  }

  function buildExtensions(fullscreen = false) {
    const updateListener = EditorView.updateListener.of(update => {
      if (update.docChanged && !isInternalUpdate) {
        const newText = update.state.doc.toString();
        const parsed = tryParseModelMapping(newText);
        if (!parsed.ok) {
          setError(parsed.error);
          return;
        }
        setError(null);
        onChange(parsed.value);
      }
    });

    const blurHandler = EditorView.domEventHandlers({
      blur: () => {
        formatIfValid();
      },
    });

    const exts = [
      lineNumbers(),
      highlightActiveLine(),
      highlightSpecialChars(),
      history(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      highlightSelectionMatches(),
      EditorState.tabSize.of(2),
      keymap.of([
        ...closeBracketsKeymap,
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
      ]),
      json(),
      darkTheme,
      EditorView.lineWrapping,
      updateListener,
      blurHandler,
    ];

    if (fullscreen) {
      exts.push(
        EditorView.theme({
          '&': { height: '100%' },
          '.cm-scroller': { overflow: 'auto' },
        })
      );
    }

    return exts;
  }

  function mountEditor(container: HTMLDivElement) {
    view?.destroy();
    const state = EditorState.create({
      doc: toPrettyJson(value),
      extensions: buildExtensions(),
    });
    view = new EditorView({ state, parent: container });
  }

  onMount(() => {
    mountEditor(editorContainer);
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });

  // Re-mount editor when toggling fullscreen
  $effect(() => {
    if (maximized && fullscreenEditorContainer) {
      const doc = view?.state.doc.toString() ?? toPrettyJson(value);
      view?.destroy();
      const state = EditorState.create({
        doc,
        extensions: buildExtensions(true),
      });
      view = new EditorView({ state, parent: fullscreenEditorContainer });
    } else if (!maximized && editorContainer) {
      const doc = view?.state.doc.toString() ?? toPrettyJson(value);
      view?.destroy();
      const state = EditorState.create({
        doc,
        extensions: buildExtensions(false),
      });
      view = new EditorView({ state, parent: editorContainer });
    }
  });

  // Sync external value changes into the editor
  $effect(() => {
    if (!view) return;
    const pretty = toPrettyJson(value);
    if (pretty !== view.state.doc.toString()) {
      isInternalUpdate = true;
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: pretty,
        },
      });
      isInternalUpdate = false;
      setError(null);
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && maximized) {
      maximized = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div>
  <div class="flex items-center justify-between mb-1">
    <label for={id} class="label">{label}</label>
    <button
      type="button"
      onclick={() => (maximized = true)}
      class="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
      title="Maximize editor"
    >
      <Maximize2 class="w-3.5 h-3.5" />
    </button>
  </div>
  <div
    class="rounded-md border overflow-hidden {error
      ? 'border-red-500'
      : 'border-gray-300 dark:border-gray-600'}"
    style="height: {rows * 1.5}rem;"
    bind:this={editorContainer}
  ></div>

  {#if helperText && !error}
    <p class="helper-text">{helperText}</p>
  {/if}
  {#if error}
    <p class="mt-1 text-xs text-red-600">{error}</p>
  {/if}
</div>

{#if maximized}
  <div
    class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex flex-col p-4"
    onclick={() => (maximized = false)}
    onkeydown={e => e.key === 'Escape' && (maximized = false)}
    role="button"
    tabindex="0"
    aria-label="Close fullscreen editor"
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="flex-1 flex flex-col bg-white dark:bg-gray-900 rounded-lg overflow-hidden shadow-2xl"
      onclick={e => e.stopPropagation()}
      onkeydown={e => e.stopPropagation()}
    >
      <div
        class="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800 shrink-0"
      >
        <span class="text-sm font-medium text-gray-700 dark:text-gray-300">
          {label}
        </span>
        <button
          type="button"
          onclick={() => (maximized = false)}
          class="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
          title="Exit fullscreen (Esc)"
        >
          <Minimize2 class="w-4 h-4" />
        </button>
      </div>
      <div
        class="flex-1 min-h-0"
        bind:this={fullscreenEditorContainer}
        style="height: calc(100vh - 7rem);"
      ></div>
      {#if error}
        <div
          class="px-4 py-2 border-t border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20"
        >
          <p class="text-xs text-red-600">{error}</p>
        </div>
      {/if}
    </div>
  </div>
{/if}
