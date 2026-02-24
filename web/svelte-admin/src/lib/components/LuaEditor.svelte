<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { auth } from '$lib/stores';
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    highlightSpecialChars,
  } from '@codemirror/view';
  import { EditorState } from '@codemirror/state';
  import {
    StreamLanguage,
    indentOnInput,
    bracketMatching,
  } from '@codemirror/language';
  import { lua } from '@codemirror/legacy-modes/mode/lua';
  import {
    defaultKeymap,
    indentWithTab,
    history,
    historyKeymap,
  } from '@codemirror/commands';
  import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
  import { highlightSelectionMatches } from '@codemirror/search';

  interface Props {
    id: string;
    label: string;
    value: string;
    onChange: (next: string) => void;
    rows?: number;
    providerId?: number | null;
  }

  let {
    id,
    label,
    value,
    onChange,
    rows = 10,
    providerId = null,
  }: Props = $props();

  let validationError = $state<string | null>(null);
  let validating = $state(false);
  let editorContainer: HTMLDivElement;
  let view: EditorView | null = null;

  const LUA_TEMPLATE = `-- Available hooks: on_request, on_response, on_stream_chunk
-- Each hook receives a context object (ctx) with methods:
--   ctx:get_request()  / ctx:set_request(tbl)
--   ctx:get_response() / ctx:set_response(tbl)
--   ctx:get_provider()
--   ctx:get_model()
--   ctx:get_meta(key) / ctx:set_meta(key, value)

function on_request(ctx)
  local req = ctx:get_request()
  -- modify req here
  ctx:set_request(req)
end

function on_response(ctx)
  local resp = ctx:get_response()
  -- modify resp here
  ctx:set_response(resp)
end
`;

  const luaLang = StreamLanguage.define(lua);

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

  function createEditor() {
    if (!editorContainer) return;

    const updateListener = EditorView.updateListener.of(update => {
      if (update.docChanged) {
        const newValue = update.state.doc.toString();
        onChange(newValue);
        validationError = null;
      }
    });

    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        highlightActiveLine(),
        highlightSpecialChars(),
        history(),
        indentOnInput(),
        bracketMatching(),
        closeBrackets(),
        highlightSelectionMatches(),
        keymap.of([
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...historyKeymap,
          indentWithTab,
        ]),
        luaLang,
        darkTheme,
        EditorView.lineWrapping,
        updateListener,
      ],
    });

    view = new EditorView({
      state,
      parent: editorContainer,
    });
  }

  onMount(() => {
    createEditor();
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });

  // Sync external value changes into the editor
  $effect(() => {
    if (view && value !== view.state.doc.toString()) {
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: value,
        },
      });
    }
  });

  function insertTemplate() {
    onChange(LUA_TEMPLATE);
    validationError = null;
  }

  function clearScript() {
    onChange('');
    validationError = null;
  }

  async function validateScript() {
    if (!value.trim() || !providerId) return;
    const client = auth.apiClient;
    if (!client) return;

    validating = true;
    validationError = null;
    try {
      const result = await client.validateScript(providerId, value);
      if (!result.valid) {
        validationError = result.error || 'Invalid script';
      }
    } catch (e) {
      validationError = e instanceof Error ? e.message : 'Validation failed';
    } finally {
      validating = false;
    }
  }
</script>

<div>
  <div class="flex items-center justify-between mb-1">
    <label for={id} class="label">{label}</label>
    <div class="flex gap-2">
      {#if value && providerId}
        <button
          type="button"
          class="text-xs text-green-600 hover:text-green-800 underline"
          onclick={validateScript}
          disabled={validating}
        >
          {validating ? 'Validating...' : 'Validate'}
        </button>
      {/if}
      {#if !value}
        <button
          type="button"
          class="text-xs text-blue-600 hover:text-blue-800 underline"
          onclick={insertTemplate}
        >
          Insert Template
        </button>
      {:else}
        <button
          type="button"
          class="text-xs text-red-600 hover:text-red-800 underline"
          onclick={clearScript}
        >
          Clear
        </button>
      {/if}
    </div>
  </div>
  <div
    class="rounded-md border overflow-hidden {validationError
      ? 'border-red-500'
      : 'border-gray-300 dark:border-gray-600'}"
    style="height: {rows * 1.5}rem;"
    bind:this={editorContainer}
  ></div>
  {#if validationError}
    <p class="text-xs text-red-600 mt-1">{validationError}</p>
  {:else}
    <p class="helper-text">
      Define hooks: on_request(ctx), on_response(ctx), on_stream_chunk(ctx)
    </p>
  {/if}
</div>
