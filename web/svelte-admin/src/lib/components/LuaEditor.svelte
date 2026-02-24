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
  import { Maximize2, Minimize2 } from 'lucide-svelte';

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
  let maximized = $state(false);
  let editorContainer: HTMLDivElement;
  let fullscreenEditorContainer: HTMLDivElement;
  let view: EditorView | null = null;

  const LUA_TEMPLATE = `-- ======================================================================
-- Raw JSON hooks: modify the final provider request / response
-- ======================================================================
-- ctx methods:
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

-- function on_stream_chunk(ctx)
--   local chunk = ctx:get_response()
--   -- modify streaming chunk here
--   ctx:set_response(chunk)
-- end

-- ======================================================================
-- Protocol transform hooks: override hardcoded protocol conversion
-- ======================================================================
-- Additional ctx methods for transform hooks:
--   ctx:get_unified()  / ctx:set_unified(tbl)
--   ctx:get_client_protocol()
--   ctx:get_provider_protocol()
--
-- Hook chain:
--   Client JSON  --[on_transform_request_out]--> UIF
--                --[on_transform_request_in]-->  Provider JSON
--   Provider JSON --[on_transform_response_in]--> UIF
--                 --[on_transform_response_out]--> Client JSON
--
-- Return without calling set_unified / set_request / set_response
-- to fall back to the hardcoded transformer for that step.

-- function on_transform_request_out(ctx)
--   local raw = ctx:get_request()
--   -- convert client raw JSON -> UIF table
--   ctx:set_unified({ model = raw.model, messages = {}, parameters = {} })
-- end

-- function on_transform_request_in(ctx)
--   local uif = ctx:get_unified()
--   -- convert UIF table -> provider raw JSON
--   ctx:set_request({ contents = {} })
-- end

-- function on_transform_response_in(ctx)
--   local raw = ctx:get_response()
--   -- convert provider raw JSON -> UIF table
--   ctx:set_unified({ id = "", model = "", content = {}, usage = {} })
-- end

-- function on_transform_response_out(ctx)
--   local uif = ctx:get_unified()
--   -- convert UIF table -> client raw JSON
--   ctx:set_response({ choices = {} })
-- end
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

  function buildExtensions(fullscreen = false) {
    const updateListener = EditorView.updateListener.of(update => {
      if (update.docChanged) {
        const newValue = update.state.doc.toString();
        onChange(newValue);
        validationError = null;
      }
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

  onMount(() => {
    const state = EditorState.create({
      doc: value,
      extensions: buildExtensions(),
    });
    view = new EditorView({ state, parent: editorContainer });
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });

  // Re-mount editor when toggling fullscreen
  $effect(() => {
    if (maximized && fullscreenEditorContainer) {
      const doc = view?.state.doc.toString() ?? value;
      view?.destroy();
      const state = EditorState.create({
        doc,
        extensions: buildExtensions(true),
      });
      view = new EditorView({ state, parent: fullscreenEditorContainer });
    } else if (!maximized && editorContainer) {
      const doc = view?.state.doc.toString() ?? value;
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

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && maximized) {
      maximized = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#snippet actionButtons()}
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
{/snippet}

<div>
  <div class="flex items-center justify-between mb-1">
    <label for={id} class="label">{label}</label>
    <div class="flex items-center gap-2">
      {@render actionButtons()}
      <button
        type="button"
        onclick={() => (maximized = true)}
        class="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
        title="Maximize editor"
      >
        <Maximize2 class="w-3.5 h-3.5" />
      </button>
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
      Hooks: on_request, on_response, on_stream_chunk,
      on_transform_request_out/in, on_transform_response_in/out
    </p>
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
        <div class="flex items-center gap-3">
          {@render actionButtons()}
          <button
            type="button"
            onclick={() => (maximized = false)}
            class="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
            title="Exit fullscreen (Esc)"
          >
            <Minimize2 class="w-4 h-4" />
          </button>
        </div>
      </div>
      <div
        class="flex-1 min-h-0"
        bind:this={fullscreenEditorContainer}
        style="height: calc(100vh - 7rem);"
      ></div>
      {#if validationError}
        <div
          class="px-4 py-2 border-t border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20"
        >
          <p class="text-xs text-red-600">{validationError}</p>
        </div>
      {/if}
    </div>
  </div>
{/if}
