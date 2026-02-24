import React, { useState, useCallback } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { StreamLanguage } from '@codemirror/language';
import { lua } from '@codemirror/legacy-modes/mode/lua';
import { useAuth } from '../hooks/useAuth';

interface LuaEditorProps {
  id: string;
  label: string;
  value: string;
  onChange: (next: string) => void;
  providerId?: number | null;
}

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

  const handleChange = useCallback(
    (val: string) => {
      onChange(val);
      setValidationError(null);
    },
    [onChange]
  );

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

  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <label htmlFor={id} className="label">
          {label}
        </label>
        <div className="flex gap-2">
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
      </div>
      <div
        className={`rounded-md border ${validationError ? 'border-red-500' : 'border-gray-300 dark:border-gray-600'} overflow-hidden`}
      >
        <CodeMirror
          id={id}
          value={value}
          height="400px"
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
      {validationError ? (
        <p className="text-xs text-red-600 mt-1">{validationError}</p>
      ) : (
        <p className="helper-text">
          Define hooks: on_request(ctx), on_response(ctx), on_stream_chunk(ctx)
        </p>
      )}
    </div>
  );
};

export default LuaEditor;
