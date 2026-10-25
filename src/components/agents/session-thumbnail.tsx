import { FileText } from "lucide-react";
import { useState } from "react";

/** A refused/unsupported decode remains a file chip, never a broken-image tile. */
export function SessionThumbnail({
  src,
  name,
  className = "size-16",
}: {
  src: string;
  name: string;
  className?: string;
}) {
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  return failedUrl === src ? (
    <FileText aria-hidden="true" className="size-5 shrink-0 text-muted-foreground" />
  ) : (
    <img
      src={src}
      alt={`Thumbnail of ${name}`}
      width={256}
      height={256}
      onError={() => setFailedUrl(src)}
      className={`${className} shrink-0 rounded object-cover`}
    />
  );
}
