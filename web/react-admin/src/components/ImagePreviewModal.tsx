import { useEffect } from 'react';
import { X, ChevronLeft, ChevronRight } from 'lucide-react';
import type { ImageAttachment } from '../types';

type Props = {
  images: ImageAttachment[];
  currentIndex: number;
  onClose: () => void;
  onNavigate: (index: number) => void;
};

export function ImagePreviewModal({
  images,
  currentIndex,
  onClose,
  onNavigate,
}: Props) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      } else if (e.key === 'ArrowLeft' && currentIndex > 0) {
        onNavigate(currentIndex - 1);
      } else if (e.key === 'ArrowRight' && currentIndex < images.length - 1) {
        onNavigate(currentIndex + 1);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [currentIndex, images.length, onClose, onNavigate]);

  if (!images[currentIndex]) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <button
        type="button"
        className="absolute inset-0 bg-black/80"
        aria-label="Close preview"
        onClick={onClose}
      />

      {/* Modal content */}
      <div className="relative z-10 max-w-[90vw] max-h-[90vh] flex flex-col items-center">
        {/* Close button */}
        <button
          type="button"
          className="absolute -top-12 right-0 p-2 text-white hover:text-gray-300 transition-colors"
          onClick={onClose}
          aria-label="Close preview"
        >
          <X className="w-6 h-6" />
        </button>

        {/* Image display */}
        <img
          src={images[currentIndex].dataUrl}
          alt={images[currentIndex].name}
          className="max-w-full max-h-[90vh] object-contain rounded-lg"
        />

        {/* Image counter */}
        {images.length > 1 && (
          <div className="mt-4 px-3 py-1 rounded-full bg-black/60 text-white text-sm">
            {currentIndex + 1} / {images.length}
          </div>
        )}

        {/* Navigation buttons */}
        {images.length > 1 && (
          <>
            {currentIndex > 0 && (
              <button
                type="button"
                className="absolute left-4 top-1/2 -translate-y-1/2 p-2 rounded-full bg-black/60 text-white hover:bg-black/80 transition-colors"
                onClick={() => onNavigate(currentIndex - 1)}
                aria-label="Previous image"
              >
                <ChevronLeft className="w-6 h-6" />
              </button>
            )}
            {currentIndex < images.length - 1 && (
              <button
                type="button"
                className="absolute right-4 top-1/2 -translate-y-1/2 p-2 rounded-full bg-black/60 text-white hover:bg-black/80 transition-colors"
                onClick={() => onNavigate(currentIndex + 1)}
                aria-label="Next image"
              >
                <ChevronRight className="w-6 h-6" />
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}
