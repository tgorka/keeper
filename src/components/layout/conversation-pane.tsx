import { PanelRight } from "lucide-react";
import type { Ref } from "react";
import { Button } from "@/components/ui/button";

interface ConversationPaneProps {
  detailOpen: boolean;
  onToggleDetail: () => void;
  toggleRef?: Ref<HTMLButtonElement>;
}

export function ConversationPane({ detailOpen, onToggleDetail, toggleRef }: ConversationPaneProps) {
  return (
    <main className="flex h-full min-w-0 flex-1 flex-col bg-background">
      <div className="flex shrink-0 items-center justify-end border-border border-b p-2">
        <Button
          ref={toggleRef}
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Toggle detail panel"
          aria-pressed={detailOpen}
          onClick={onToggleDetail}
          className="focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <PanelRight aria-hidden="true" />
        </Button>
      </div>
      <div className="flex flex-1 items-center justify-center p-8">
        <p className="max-w-sm text-center text-muted-foreground text-sm">
          Select a conversation to start reading.
        </p>
      </div>
    </main>
  );
}
