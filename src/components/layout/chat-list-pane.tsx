export function ChatListPane() {
  return (
    <div className="flex h-full w-[320px] shrink-0 flex-col border-border border-r bg-background">
      <ul aria-label="Conversations" className="flex flex-1 items-center justify-center p-4">
        <li className="text-center text-muted-foreground text-sm">Synced. No conversations yet.</li>
      </ul>
    </div>
  );
}
