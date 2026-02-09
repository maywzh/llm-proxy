import {
  ImagePlus,
  Loader2,
  Send,
  Settings,
  StopCircle,
  X,
} from 'lucide-react';
import React, { useRef, useEffect, useState } from 'react';
import type { ImageAttachment } from '../types';
import { ImagePreviewModal } from './ImagePreviewModal';

type ModelOption = {
  value: string;
  label: string;
};

type Props = {
  input: string;
  onInputChange: (value: string) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onPaste: (e: React.ClipboardEvent<HTMLTextAreaElement>) => void;

  isLoading: boolean;
  isStreaming: boolean;

  onSend: () => void;
  sendDisabled: boolean;

  onStop: () => void;

  images: ImageAttachment[];
  onRemoveImage: (id: string) => void;
  imageError: string | null;
  imageInputRef: React.RefObject<HTMLInputElement | null>;
  onImageChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  onPickImage: () => void;
  showImageButton: boolean;

  selectedModel: string;
  onSelectModel: (model: string) => void;
  modelOptions: ModelOption[];

  onOpenSettings: () => void;
};

export function ChatComposer({
  input,
  onInputChange,
  onKeyDown,
  onPaste,
  isLoading,
  isStreaming,
  onSend,
  sendDisabled,
  onStop,
  images,
  onRemoveImage,
  imageError,
  imageInputRef,
  onImageChange,
  onPickImage,
  showImageButton,
  selectedModel,
  onSelectModel,
  modelOptions,
  onOpenSettings,
}: Props) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  }, [input]);

  return (
    <div className="px-4 py-3">
      <div className="mx-auto w-full max-w-3xl">
        {imageError ? (
          <p className="mb-2 text-sm text-red-600 dark:text-red-400">
            {imageError}
          </p>
        ) : null}

        <div className="rounded-2xl bg-gray-50 dark:bg-gray-700/30 ring-1 ring-gray-200/80 dark:ring-gray-600 shadow-sm">
          {images.length > 0 ? (
            <div className="px-3 pt-3">
              <div className="flex gap-2 overflow-x-auto">
                {images.map((img, index) => (
                  <div key={img.id} className="relative shrink-0">
                    <button
                      type="button"
                      onClick={() => setPreviewIndex(index)}
                      className="block"
                    >
                      <img
                        src={img.dataUrl}
                        alt={img.name}
                        className="h-16 w-16 object-cover rounded-lg border border-gray-200 dark:border-gray-600 hover:opacity-80 transition-opacity"
                      />
                    </button>
                    <button
                      type="button"
                      className="absolute -top-1.5 -right-1.5 w-5 h-5 flex items-center justify-center rounded-full bg-gray-800/80 dark:bg-gray-600/90 text-white hover:bg-gray-900 dark:hover:bg-gray-500 transition-colors shadow-sm"
                      title="Remove image"
                      onClick={() => onRemoveImage(img.id)}
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </div>
                ))}
              </div>
            </div>
          ) : null}

          <textarea
            ref={textareaRef}
            value={input}
            onChange={e => onInputChange(e.target.value)}
            onKeyDown={onKeyDown}
            onPaste={onPaste}
            placeholder="Message..."
            className="w-full bg-transparent text-gray-900 dark:text-white placeholder-gray-400 dark:placeholder-gray-500 focus:outline-none resize-none px-4 py-3 min-h-11"
            rows={1}
            disabled={isLoading}
          />

          <div className="flex items-center justify-between gap-2 px-3 pb-3">
            <div className="flex items-center gap-1.5">
              <input
                ref={imageInputRef}
                type="file"
                accept="image/*"
                onChange={onImageChange}
                className="hidden"
              />
              {showImageButton ? (
                <button
                  type="button"
                  onClick={onPickImage}
                  disabled={isLoading}
                  className="h-8 w-8 inline-flex items-center justify-center rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-200/60 dark:hover:bg-gray-600/50 focus:outline-none disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  title="Attach image"
                  aria-label="Attach image"
                >
                  <ImagePlus className="w-4 h-4" />
                </button>
              ) : null}
            </div>

            <div className="flex items-center gap-2">
              <select
                value={selectedModel}
                onChange={e => onSelectModel(e.target.value)}
                disabled={isLoading || modelOptions.length === 0}
                className="h-8 w-36 sm:w-48 bg-transparent text-gray-500 dark:text-gray-400 rounded-lg focus:ring-1 focus:ring-primary-500 focus:outline-none px-2 text-xs border-none"
                title="Model"
              >
                {modelOptions.length === 0 ? (
                  <option value="">Set credential key</option>
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
                className="h-8 w-8 inline-flex items-center justify-center rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-200/60 dark:hover:bg-gray-600/50 focus:outline-none transition-colors"
                title="Settings"
                aria-label="Settings"
              >
                <Settings className="w-4 h-4" />
              </button>

              {isStreaming ? (
                <button
                  onClick={onStop}
                  className="h-8 w-8 inline-flex items-center justify-center rounded-lg bg-red-600 text-white hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-500 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  title="Stop"
                  aria-label="Stop generation"
                >
                  <StopCircle className="w-4 h-4" />
                </button>
              ) : (
                <button
                  onClick={onSend}
                  disabled={sendDisabled}
                  className="h-8 w-8 inline-flex items-center justify-center rounded-lg bg-primary-600 text-white hover:bg-primary-700 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                  title="Send"
                  aria-label="Send message"
                >
                  {isLoading ? (
                    <Loader2 className="w-4 h-4 animate-spin" />
                  ) : (
                    <Send className="w-4 h-4" />
                  )}
                </button>
              )}
            </div>
          </div>
        </div>
      </div>

      {previewIndex !== null && images.length > 0 && (
        <ImagePreviewModal
          images={images}
          currentIndex={previewIndex}
          onClose={() => setPreviewIndex(null)}
          onNavigate={setPreviewIndex}
        />
      )}
    </div>
  );
}
