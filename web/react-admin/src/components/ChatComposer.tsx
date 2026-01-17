import {
  ImagePlus,
  Loader2,
  Send,
  Settings,
  StopCircle,
  X,
} from 'lucide-react';
import React from 'react';

type ModelOption = {
  value: string;
  label: string;
};

type Props = {
  input: string;
  onInputChange: (value: string) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;

  isLoading: boolean;
  isStreaming: boolean;

  onSend: () => void;
  sendDisabled: boolean;

  onStop: () => void;

  imageDataUrl: string | null;
  onRemoveImage: () => void;
  imageError: string | null;
  imageInputRef: React.RefObject<HTMLInputElement | null>;
  onImageChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  onPickImage: () => void;
  attachImageDisabled: boolean;
  attachImageTitle: string;

  selectedModel: string;
  onSelectModel: (model: string) => void;
  modelOptions: ModelOption[];

  onOpenSettings: () => void;
};

export function ChatComposer({
  input,
  onInputChange,
  onKeyDown,
  isLoading,
  isStreaming,
  onSend,
  sendDisabled,
  onStop,
  imageDataUrl,
  onRemoveImage,
  imageError,
  imageInputRef,
  onImageChange,
  onPickImage,
  attachImageDisabled,
  attachImageTitle,
  selectedModel,
  onSelectModel,
  modelOptions,
  onOpenSettings,
}: Props) {
  return (
    <div className="px-4 py-3">
      <div className="mx-auto w-full max-w-3xl">
        {imageDataUrl ? (
          <div className="mb-3 flex items-center space-x-3">
            <img
              src={imageDataUrl}
              alt="preview"
              className="h-16 w-16 object-cover rounded-lg border border-gray-200 dark:border-gray-600"
            />
            <button
              type="button"
              className="btn btn-secondary"
              title="Remove image"
              onClick={onRemoveImage}
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        ) : null}

        {imageError ? (
          <p className="mb-3 text-sm text-red-600 dark:text-red-400">
            {imageError}
          </p>
        ) : null}

        <div className="rounded-2xl bg-gray-50 dark:bg-gray-700/30 ring-1 ring-gray-200/80 dark:ring-gray-600 shadow-sm p-3">
          <textarea
            value={input}
            onChange={e => onInputChange(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Type your message... (Press Enter to send, Shift+Enter for new line)"
            className="w-full bg-transparent text-gray-900 dark:text-white placeholder-gray-400 dark:placeholder-gray-400 focus:outline-none resize-none"
            rows={3}
            disabled={isLoading}
          />

          <div className="mt-2 flex items-center justify-between gap-2">
            <div className="flex items-center space-x-2">
              <input
                ref={imageInputRef}
                type="file"
                accept="image/*"
                onChange={onImageChange}
                className="hidden"
              />
              <button
                type="button"
                onClick={onPickImage}
                disabled={attachImageDisabled}
                className="h-10 w-10 inline-flex items-center justify-center rounded-full bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-50 disabled:cursor-not-allowed"
                title={attachImageTitle}
              >
                <ImagePlus className="w-5 h-5" />
              </button>
            </div>

            <div className="flex items-center gap-2">
              <select
                value={selectedModel}
                onChange={e => onSelectModel(e.target.value)}
                disabled={isLoading || modelOptions.length === 0}
                className="h-10 w-40 sm:w-56 bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 text-gray-900 dark:text-white rounded-full focus:ring-2 focus:ring-primary-500 focus:outline-none px-3 text-sm"
                title="Model"
              >
                {modelOptions.length === 0 ? (
                  <option value="">Set credential key in Settings</option>
                ) : null}
                {modelOptions.map(model => (
                  <option key={model.value} value={model.value}>
                    {model.label}
                  </option>
                ))}
              </select>

              <button
                type="button"
                onClick={onOpenSettings}
                className="h-10 w-10 inline-flex items-center justify-center rounded-full bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700 focus:outline-none focus:ring-2 focus:ring-primary-500"
                title="Settings (set credential key)"
                aria-label="Settings"
              >
                <Settings className="w-5 h-5" />
              </button>

              {isStreaming ? (
                <button
                  onClick={onStop}
                  className="h-10 w-10 inline-flex items-center justify-center rounded-full bg-red-600 text-white hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-500 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Stop Generation"
                >
                  <StopCircle className="w-5 h-5" />
                </button>
              ) : (
                <button
                  onClick={onSend}
                  disabled={sendDisabled}
                  className="h-10 w-10 inline-flex items-center justify-center rounded-full bg-primary-600 text-white hover:bg-primary-700 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Send Message"
                >
                  {isLoading ? (
                    <Loader2 className="w-5 h-5 animate-spin" />
                  ) : (
                    <Send className="w-5 h-5" />
                  )}
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
