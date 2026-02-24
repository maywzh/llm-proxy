import React, { useCallback, useEffect, useRef, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import type { ModelMappingEntry, ModelMappingValue } from '../types';

type JsonEditorProps = {
  id: string;
  label: string;
  value: Record<string, ModelMappingValue>;
  onChange: (next: Record<string, ModelMappingValue>) => void;
  onErrorChange?: (error: string | null) => void;
  placeholder?: string;
  helperText?: string;
  rows?: number;
};

function toPrettyJson(value: Record<string, ModelMappingValue>): string {
  return JSON.stringify(value ?? {}, null, 2);
}

function tryParseModelMapping(
  text: string
):
  | { ok: true; value: Record<string, ModelMappingValue> }
  | { ok: false; error: string } {
  const trimmed = text.trim();
  if (!trimmed) return { ok: true, value: {} };

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return { ok: false, error: 'Invalid JSON' };
  }

  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return { ok: false, error: 'JSON must be an object' };
  }

  const mapping: Record<string, ModelMappingValue> = {};
  for (const [key, value] of Object.entries(
    parsed as Record<string, unknown>
  )) {
    if (typeof value === 'string') {
      mapping[key] = value;
    } else if (
      typeof value === 'object' &&
      value !== null &&
      !Array.isArray(value)
    ) {
      const entry = value as Record<string, unknown>;
      if (typeof entry.mapped_model !== 'string') {
        return {
          ok: false,
          error: `Entry "${key}" must have a string "mapped_model" field`,
        };
      }
      mapping[key] = entry as unknown as ModelMappingEntry;
    } else {
      return {
        ok: false,
        error: `Value for "${key}" must be a string or an object with mapped_model`,
      };
    }
  }

  return { ok: true, value: mapping };
}

const jsonLang = json();

const JsonEditor: React.FC<JsonEditorProps> = ({
  id,
  label,
  value,
  onChange,
  onErrorChange,
  placeholder,
  helperText,
  rows = 6,
}) => {
  const [text, setText] = useState(() => toPrettyJson(value));
  const [error, setError] = useState<string | null>(null);
  const isEditingRef = useRef(false);

  const setErrorAndNotify = useCallback(
    (next: string | null) => {
      setError(next);
      onErrorChange?.(next);
    },
    [onErrorChange]
  );

  useEffect(() => {
    if (isEditingRef.current) return;
    setText(toPrettyJson(value));
    setErrorAndNotify(null);
  }, [setErrorAndNotify, value]);

  const handleChange = useCallback(
    (nextText: string) => {
      isEditingRef.current = true;
      setText(nextText);

      const parsed = tryParseModelMapping(nextText);
      if (!parsed.ok) {
        setErrorAndNotify(parsed.error);
        return;
      }

      setErrorAndNotify(null);
      onChange(parsed.value);
    },
    [onChange, setErrorAndNotify]
  );

  const handleBlur = useCallback(() => {
    isEditingRef.current = false;

    const parsed = tryParseModelMapping(text);
    if (!parsed.ok) return;

    setText(toPrettyJson(parsed.value));
  }, [text]);

  const height = `${rows * 1.5}rem`;

  return (
    <div>
      <label htmlFor={id} className="label">
        {label}
      </label>
      <div
        className={`rounded-md border ${error ? 'border-red-500' : 'border-gray-300 dark:border-gray-600'} overflow-hidden`}
      >
        <CodeMirror
          id={id}
          value={text}
          height={height}
          extensions={[jsonLang]}
          onChange={handleChange}
          onBlur={handleBlur}
          theme="dark"
          placeholder={placeholder}
          basicSetup={{
            lineNumbers: true,
            foldGutter: false,
            highlightActiveLine: true,
            tabSize: 2,
          }}
        />
      </div>
      {helperText && !error && <p className="helper-text">{helperText}</p>}
      {error && <p className="mt-1 text-xs text-red-600">{error}</p>}
    </div>
  );
};

export default JsonEditor;
