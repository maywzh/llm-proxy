import React, { useState, useCallback, useEffect } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { StreamLanguage } from '@codemirror/language';
import { lua } from '@codemirror/legacy-modes/mode/lua';
import { Maximize2, Minimize2 } from 'lucide-react';
import { useAuth } from '../hooks/useAuth';

interface LuaEditorProps {
  id: string;
  label: string;
  value: string;
  onChange: (next: string) => void;
  providerId?: number | null;
}

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

const LuaEditor: React.FC<LuaEditorProps> = ({
  id,
  label,
  value,
  onChange,
  providerId = null,
}) => {
  const { apiClient } = useAuth();
  const [validationError, setValidationError] = useState<string | null>(null);
  const [validating, setValidating] = useState(false);
  const [maximized, setMaximized] = useState(false);

  const handleChange = useCallback(
    (val: string) => {
      onChange(val);
      setValidationError(null);
    },
    [onChange]
  );

  useEffect(() => {
    if (!maximized) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMaximized(false);
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [maximized]);

  const insertTemplate = () => {
    onChange(LUA_TEMPLATE);
    setValidationError(null);
  };

  const clearScript = () => {
    onChange('');
    setValidationError(null);
  };

  const validateScript = async () => {
    if (!value.trim() || !providerId || !apiClient) return;

    setValidating(true);
    setValidationError(null);
    try {
      const result = await apiClient.validateScript(providerId, value);
      if (!result.valid) {
        setValidationError(result.error || 'Invalid script');
      }
    } catch (e) {
      setValidationError(e instanceof Error ? e.message : 'Validation failed');
    } finally {
      setValidating(false);
    }
  };

  const actionButtons = (
    <div className="flex gap-2 items-center">
      {value && providerId && (
        <button
          type="button"
          className="text-xs text-green-600 hover:text-green-800 underline"
          onClick={validateScript}
          disabled={validating}
        >
          {validating ? 'Validating...' : 'Validate'}
        </button>
      )}
      {!value ? (
        <button
          type="button"
          className="text-xs text-blue-600 hover:text-blue-800 underline"
          onClick={insertTemplate}
        >
          Insert Template
        </button>
      ) : (
        <button
          type="button"
          className="text-xs text-red-600 hover:text-red-800 underline"
          onClick={clearScript}
        >
          Clear
        </button>
      )}
    </div>
  );

  const editorElement = (editorHeight: string) => (
    <div
      className={`rounded-md border ${validationError ? 'border-red-500' : 'border-gray-300 dark:border-gray-600'} overflow-hidden`}
    >
      <CodeMirror
        id={id}
        value={value}
        height={editorHeight}
        extensions={[luaLang]}
        onChange={handleChange}
        theme="dark"
        basicSetup={{
          lineNumbers: true,
          foldGutter: false,
          highlightActiveLine: true,
          tabSize: 2,
        }}
      />
    </div>
  );

  return (
    <>
      <div>
        <div className="flex items-center justify-between mb-1">
          <label htmlFor={id} className="label">
            {label}
          </label>
          <div className="flex items-center gap-2">
            {actionButtons}
            <button
              type="button"
              onClick={() => setMaximized(true)}
              className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
              title="Maximize editor"
            >
              <Maximize2 className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
        {editorElement('400px')}
        {validationError ? (
          <p className="text-xs text-red-600 mt-1">{validationError}</p>
        ) : (
          <p className="helper-text">
            Hooks: on_request, on_response, on_stream_chunk,
            on_transform_request_out/in, on_transform_response_in/out
          </p>
        )}
      </div>

      {maximized && (
        <div
          className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex flex-col p-4"
          onClick={() => setMaximized(false)}
        >
          <div
            className="flex-1 flex flex-col bg-white dark:bg-gray-900 rounded-lg overflow-hidden shadow-2xl"
            onClick={e => e.stopPropagation()}
          >
            <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800 shrink-0">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                {label}
              </span>
              <div className="flex items-center gap-3">
                {actionButtons}
                <button
                  type="button"
                  onClick={() => setMaximized(false)}
                  className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
                  title="Exit fullscreen (Esc)"
                >
                  <Minimize2 className="w-4 h-4" />
                </button>
              </div>
            </div>
            <div className="flex-1 min-h-0">
              {editorElement('calc(100vh - 7rem)')}
            </div>
            {validationError && (
              <div className="px-4 py-2 border-t border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20">
                <p className="text-xs text-red-600">{validationError}</p>
              </div>
            )}
          </div>
        </div>
      )}
    </>
  );
};

export default LuaEditor;
