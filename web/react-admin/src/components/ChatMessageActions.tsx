import { Check, Copy, RotateCcw, Share2 } from 'lucide-react';

type Props = {
  copied: boolean;
  disabled?: boolean;
  onCopy: () => void;

  shared?: boolean;
  shareDisabled?: boolean;
  onShare?: () => void;

  showRegenerate?: boolean;
  regenerateDisabled?: boolean;
  onRegenerate?: () => void;
};

export function ChatMessageActions({
  copied,
  disabled,
  onCopy,
  shared,
  shareDisabled,
  onShare,
  showRegenerate,
  regenerateDisabled,
  onRegenerate,
}: Props) {
  return (
    <div className="mt-1 px-4 flex items-center gap-2 text-gray-500 dark:text-gray-300 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity">
      <div className="relative group/copy">
        <button
          type="button"
          className="p-1.5 rounded-md hover:bg-black/5 dark:hover:bg-white/10 cursor-pointer disabled:cursor-not-allowed disabled:opacity-60"
          onClick={onCopy}
          disabled={disabled}
          aria-label="Copy message"
        >
          {copied ? (
            <Check className="w-4 h-4" />
          ) : (
            <Copy className="w-4 h-4" />
          )}
        </button>
        <div className="pointer-events-none absolute -top-9 left-1/2 -translate-x-1/2 whitespace-nowrap rounded-md bg-black px-2 py-1 text-xs text-white opacity-0 group-hover/copy:opacity-100 transition-opacity shadow">
          {copied ? 'Copied' : 'Copy'}
        </div>
      </div>

      <div className="relative group/share">
        <button
          type="button"
          className="p-1.5 rounded-md hover:bg-black/5 dark:hover:bg-white/10 cursor-pointer disabled:cursor-not-allowed disabled:opacity-60"
          onClick={onShare}
          disabled={shareDisabled}
          aria-label="Share as curl"
        >
          {shared ? (
            <Check className="w-4 h-4" />
          ) : (
            <Share2 className="w-4 h-4" />
          )}
        </button>
        <div className="pointer-events-none absolute -top-9 left-1/2 -translate-x-1/2 whitespace-nowrap rounded-md bg-black px-2 py-1 text-xs text-white opacity-0 group-hover/share:opacity-100 transition-opacity shadow">
          {shared ? 'Copied' : 'Share'}
        </div>
      </div>

      {showRegenerate ? (
        <div className="relative group/regen">
          <button
            type="button"
            className="p-1.5 rounded-md hover:bg-black/5 dark:hover:bg-white/10 cursor-pointer disabled:cursor-not-allowed disabled:opacity-60"
            onClick={onRegenerate}
            disabled={regenerateDisabled}
            aria-label="Regenerate"
          >
            <RotateCcw className="w-4 h-4" />
          </button>
          <div className="pointer-events-none absolute -top-9 left-1/2 -translate-x-1/2 whitespace-nowrap rounded-md bg-black px-2 py-1 text-xs text-white opacity-0 group-hover/regen:opacity-100 transition-opacity shadow">
            Regenerate
          </div>
        </div>
      ) : null}
    </div>
  );
}
