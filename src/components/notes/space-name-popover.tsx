import { useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

export const SPACE_TTL_ERROR =
  "Lifetime must be a positive whole number of hours (up to 4294967295).";
export function validSpaceTtl(value: string): boolean {
  const hours = Number(value);
  return value.trim() !== "" && Number.isInteger(hours) && hours > 0 && hours <= 4294967295;
}

/** Non-modal: naming never traps focus or suspends the search behind it. */
export function SpaceNamePopover({
  anchor,
  initialName,
  onSave,
  onClose,
}: {
  anchor: HTMLElement;
  initialName: string;
  onSave: (name: string, options: { ttlHours: number | null }) => Promise<void>;
  onClose: () => void;
}) {
  const nameId = useId();
  const { phone } = useShellLayout();
  const ttlId = useId();
  const titleId = useId();
  const input = useRef<HTMLInputElement>(null);
  const [name, setName] = useState(initialName);
  const [temporary, setTemporary] = useState(false);
  const [hours, setHours] = useState("48");
  const [saving, setSaving] = useState(false);
  const pending = useRef(false);
  const returnFocus = useRef(true);
  const [failure, setFailure] = useState<string | null>(null);
  const invalidTtl = temporary && !validSpaceTtl(hours);
  const invalid = name.trim() === "" || invalidTtl;
  async function save() {
    if (invalid || pending.current) return;
    pending.current = true;
    setSaving(true);
    setFailure(null);
    try {
      await onSave(name.trim(), { ttlHours: temporary ? Number(hours) : null });
      onClose();
    } catch (error) {
      setFailure(syncErrorMessage(error, "keeper couldn't save this space."));
    } finally {
      pending.current = false;
      setSaving(false);
    }
  }
  return (
    <Popover
      open
      modal={false}
      onOpenChange={(open) => {
        if (!open && !pending.current) onClose();
      }}
    >
      <PopoverAnchor virtualRef={{ current: anchor }} />
      <PopoverContent
        align="end"
        sideOffset={4}
        collisionPadding={8}
        aria-labelledby={titleId}
        className="max-h-[calc(100dvh-16px)] w-80 max-w-[calc(100vw-16px)] gap-2 overflow-y-auto rounded-[10px] p-3"
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          input.current?.focus();
          input.current?.select();
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (returnFocus.current) anchor.focus();
        }}
        onInteractOutside={(event) => {
          if (pending.current) event.preventDefault();
          else returnFocus.current = false;
        }}
        onEscapeKeyDown={(event) => {
          if (pending.current) event.preventDefault();
        }}
      >
        <h2 id={titleId} className="font-semibold text-title leading-5">
          Save as space
        </h2>
        <form
          className="flex flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <Label htmlFor={nameId}>Name</Label>
          <Input
            ref={input}
            id={nameId}
            value={name}
            disabled={saving}
            autoComplete="off"
            className={cn(phone ? "h-11" : "h-8")}
            onKeyDown={(event) => {
              if (event.key !== "Enter") return;
              event.preventDefault();
              if (!event.nativeEvent.isComposing) void save();
            }}
            onChange={(event) => {
              setName(event.target.value);
              setFailure(null);
            }}
          />
          <p className="text-meta text-muted-foreground leading-4">Use / to group spaces</p>
          <label className={cn("flex items-center gap-2 text-sm", phone ? "min-h-11" : "min-h-8")}>
            <input
              type="checkbox"
              checked={temporary}
              disabled={saving}
              onChange={(event) => setTemporary(event.target.checked)}
            />
            Temporary space
          </label>
          {temporary && (
            <>
              <Label htmlFor={ttlId}>Expires after inactivity (hours)</Label>
              <Input
                id={ttlId}
                type="number"
                min={1}
                max={4294967295}
                step={1}
                value={hours}
                disabled={saving}
                className={cn(phone ? "h-11" : "h-8")}
                onChange={(event) => setHours(event.target.value)}
              />
              <p className="text-meta text-muted-foreground leading-4">
                Opening this space refreshes its lifetime. Expired spaces go to Trash.
              </p>
            </>
          )}
          {(invalidTtl || failure) && (
            <p role="alert" className="text-destructive text-xs">
              {invalidTtl ? SPACE_TTL_ERROR : failure}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              disabled={saving}
              className={cn("min-w-16", phone ? "h-11" : "h-8")}
              onClick={onClose}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={invalid || saving}
              className={cn("min-w-16", phone ? "h-11" : "h-8")}
            >
              {saving ? "Saving…" : "Save"}
            </Button>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}
