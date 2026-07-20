import { useState } from "react";
import { toast } from "sonner";
import { Copy, Loader2, UserPlus, X } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createTauRPCProxy, type InviteCode } from "@/bindings";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

const cmd = () => createTauRPCProxy().cmd;

export const GRANTED_SHARES_QUERY_KEY = ["grantedShares"];

const expiresLabel = (at: number | null) =>
  at == null
    ? ""
    : ` · expires ${new Date(at).toLocaleDateString(undefined, {
        month: "short",
        day: "numeric",
      })}`;

/**
 * Share one setup: mint a code to send, and manage who already has one.
 *
 * A code is a bearer credential handed to a human over their own channel, so
 * this screen's job is to show it exactly once, clearly, and make it easy to
 * get out of the app — and to make withdrawing it just as easy, because the
 * only other way a mis-sent code stops working is waiting 48 hours.
 */
export default function ShareDialog({
  setupId,
  setupName,
  open,
  onOpenChange,
}: {
  setupId: string;
  setupName: string;
  open: boolean;
  onOpenChange: (next: boolean) => void;
}) {
  const queryClient = useQueryClient();
  const [minted, setMinted] = useState<InviteCode | null>(null);
  const [label, setLabel] = useState("");
  const [pending, setPending] = useState(false);

  const { data: shares } = useQuery({
    queryKey: GRANTED_SHARES_QUERY_KEY,
    queryFn: () => cmd().list_granted_shares(),
    enabled: open,
  });
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: GRANTED_SHARES_QUERY_KEY });

  // Only this setup's rows — the dialog is scoped to the setup it opened from.
  const contacts = shares?.granted.filter((g) => g.setupId === setupId) ?? [];
  const outstanding = shares?.pending.filter((p) => p.setupId === setupId) ?? [];

  const mint = async () => {
    setPending(true);
    try {
      setMinted(await cmd().invite_to_setup(setupId, label.trim() || null));
      setLabel("");
      await refresh();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setPending(false);
    }
  };

  const copy = async (code: string) => {
    try {
      await navigator.clipboard.writeText(code);
      toast.success("Code copied — send it however you like");
    } catch {
      // Clipboard access can be refused; the code is on screen either way,
      // which is why it is rendered large enough to read aloud or retype.
      toast.error("Couldn't copy — the code is above, ready to select");
    }
  };

  const revoke = async (contactSub: string, who: string) => {
    try {
      await cmd().revoke_share(contactSub, setupId);
      await refresh();
      toast.success(`${who} can no longer control ${setupName}`);
    } catch (e) {
      toast.error(String(e));
    }
  };

  const withdraw = async (codeRef: string) => {
    try {
      await cmd().withdraw_invite(codeRef);
      await refresh();
      if (minted?.codeRef === codeRef) setMinted(null);
      toast.success("Code withdrawn — it will no longer work");
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setMinted(null);
        onOpenChange(next);
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Share {setupName}</DialogTitle>
          <DialogDescription>
            Anyone with a code can control this setup&rsquo;s lights from their
            own account. They can&rsquo;t see your other setups or change the
            patch.
          </DialogDescription>
        </DialogHeader>

        {minted ? (
          <div className="flex flex-col gap-2 rounded-md border p-3">
            <p className="text-xs text-muted-foreground">
              Send this to one person. It works once, and expires in 48 hours.
            </p>
            <p className="select-all text-center font-mono text-lg tracking-wider">
              {minted.code}
            </p>
            <div className="flex gap-2">
              <Button size="sm" className="flex-1" onClick={() => copy(minted.code)}>
                <Copy className="size-4" /> Copy
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setMinted(null)}>
                Done
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex items-center gap-2">
            <Input
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void mint();
              }}
              // The owner's own note, never shown to the contact — it exists so
              // the list below reads as people rather than email addresses.
              placeholder="Who's it for? (optional)"
              className="h-9"
              maxLength={120}
            />
            <Button size="sm" onClick={mint} disabled={pending}>
              {pending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <>
                  <UserPlus className="size-4" /> New code
                </>
              )}
            </Button>
          </div>
        )}

        {contacts.length > 0 && (
          <ul className="flex flex-col gap-1">
            {contacts.map((c) => (
              <li
                key={c.contactSub}
                className="flex items-center justify-between gap-2 rounded-md border px-3 py-2"
              >
                <span className="flex min-w-0 flex-col">
                  <span className="truncate text-sm">
                    {c.label ?? c.contactLabel}
                  </span>
                  {c.label ? (
                    <span className="truncate text-xs text-muted-foreground">
                      {c.contactLabel}
                    </span>
                  ) : null}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => revoke(c.contactSub, c.label ?? c.contactLabel)}
                >
                  Remove
                </Button>
              </li>
            ))}
          </ul>
        )}

        {outstanding.length > 0 && (
          <div className="flex flex-col gap-1">
            <p className="text-xs font-medium text-muted-foreground">
              Unclaimed codes
            </p>
            {outstanding.map((p) => (
              <div
                key={p.codeRef}
                className="flex items-center justify-between gap-2 rounded-md border px-3 py-2"
              >
                <span className="truncate text-sm text-muted-foreground">
                  {p.label ?? "No note"}
                  {/* `f64` crosses as `number | null` (a non-finite value has
                      no JSON form), so the expiry is shown only when it is
                      actually a date. */}
                  {expiresLabel(p.expiresAt)}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label="Withdraw this code"
                  onClick={() => withdraw(p.codeRef)}
                >
                  <X className="size-4" />
                </Button>
              </div>
            ))}
          </div>
        )}

        {contacts.length === 0 && outstanding.length === 0 && !minted && (
          <p className="text-sm text-muted-foreground">
            Nobody has access to this setup yet.
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}
