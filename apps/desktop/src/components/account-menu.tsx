import { useState } from "react";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { LogOut, Trash2, User as UserIcon } from "lucide-react";
import { createTauRPCProxy } from "@/bindings";
import useAuth, { AUTH_QUERY_KEY } from "@/hooks/useAuth";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";

const cmd = () => createTauRPCProxy().cmd;

type Mode = "signIn" | "signUp" | "confirm";

/**
 * Account control in the nav. Hidden entirely when accounts aren't configured
 * (no `COGNITO_*`), so the local-only app is unchanged. Signed in: a dropdown
 * with the email, sign out, and account deletion (behind a confirm dialog —
 * removes the account and its cloud data; local setups stay on the device).
 * Signed out: a dialog with sign-in / sign-up / email-confirmation. State
 * updates reactively from the `authChanged` event, so a successful sign-in or
 * deletion just closes its dialog.
 */
export default function AccountMenu() {
  const status = useAuth();
  const queryClient = useQueryClient();
  // The authoritative post-action refresh: refetch account status right after a
  // sign-in / sign-out / delete rather than waiting on the `authChanged` event
  // (which iOS drops), so the UI flips immediately.
  const refreshAuth = () =>
    queryClient.invalidateQueries({ queryKey: AUTH_QUERY_KEY });
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<Mode>("signIn");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [pending, setPending] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);

  // Accounts disabled (or status still loading) — render nothing.
  if (!status?.configured) return null;

  if (status.signedIn) {
    const signOut = () =>
      cmd()
        .sign_out()
        .then(refreshAuth)
        .catch((e) => toast.error(String(e)));
    const deleteAccount = async () => {
      setDeleting(true);
      try {
        await cmd().delete_account();
        await refreshAuth();
        setConfirmDelete(false);
        toast.success("Account deleted. Your setups are still on this device.");
      } catch (err) {
        toast.error(String(err));
      } finally {
        setDeleting(false);
      }
    };
    return (
      <>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            {/* Just the avatar on narrow screens (the email would overflow the
                nav); avatar + email once there's room. The email is always in
                the menu below. */}
            <Button
              variant="outline"
              size="sm"
              className="gap-2 px-1.5 sm:pr-3"
              aria-label={status.email ?? "Account"}
            >
              <Avatar className="size-6">
                <AvatarFallback className="bg-transparent text-xs font-medium uppercase">
                  {status.email?.[0] ?? <UserIcon className="size-3.5" />}
                </AvatarFallback>
              </Avatar>
              <span className="hidden max-w-32 truncate sm:inline">
                {status.email ?? "Account"}
              </span>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-56">
            <DropdownMenuLabel className="truncate font-normal text-muted-foreground">
              {status.email}
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={signOut} className="gap-2">
              <LogOut className="size-4" /> Sign out
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onSelect={() => setConfirmDelete(true)}
              className="gap-2 text-destructive focus:text-destructive"
            >
              <Trash2 className="size-4" /> Delete account…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

        <Dialog open={confirmDelete} onOpenChange={setConfirmDelete}>
          <DialogContent className="sm:max-w-sm">
            <DialogHeader>
              <DialogTitle>Delete account?</DialogTitle>
              <DialogDescription>
                This permanently deletes your account and its cloud-synced
                setups. Setups on this device stay on this device. This can't
                be undone.
              </DialogDescription>
            </DialogHeader>
            <div className="flex justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => setConfirmDelete(false)}
                disabled={deleting}
              >
                Cancel
              </Button>
              <Button
                variant="destructive"
                onClick={deleteAccount}
                disabled={deleting}
              >
                {deleting ? "…" : "Delete account"}
              </Button>
            </div>
          </DialogContent>
        </Dialog>
      </>
    );
  }

  const go = (m: Mode) => {
    setMode(m);
    setPassword("");
    setCode("");
  };

  const onOpenChange = (next: boolean) => {
    setOpen(next);
    if (next) go("signIn");
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setPending(true);
    try {
      const addr = email.trim();
      if (mode === "signIn") {
        await cmd().sign_in(addr, password);
        await refreshAuth();
        setOpen(false);
      } else if (mode === "signUp") {
        await cmd().sign_up(addr, password);
        toast.success("Check your email for a confirmation code.");
        go("confirm");
      } else {
        await cmd().confirm_sign_up(addr, code.trim());
        toast.success("Email confirmed — now sign in.");
        go("signIn");
      }
    } catch (err) {
      toast.error(String(err));
    } finally {
      setPending(false);
    }
  };

  const title =
    mode === "signUp"
      ? "Create account"
      : mode === "confirm"
        ? "Confirm email"
        : "Sign in";
  const cta = mode === "confirm" ? "Confirm" : title;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          Sign in
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>
            {mode === "confirm"
              ? "Enter the code we emailed you."
              : "Your setups sync across devices when you're signed in."}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={submit} className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="account-email">Email</Label>
            <Input
              id="account-email"
              type="email"
              autoComplete="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              disabled={pending || mode === "confirm"}
            />
          </div>

          {mode !== "confirm" ? (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="account-password">Password</Label>
              <Input
                id="account-password"
                type="password"
                autoComplete={mode === "signUp" ? "new-password" : "current-password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                disabled={pending}
              />
            </div>
          ) : (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="account-code">Confirmation code</Label>
              <Input
                id="account-code"
                inputMode="numeric"
                autoComplete="one-time-code"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                required
                disabled={pending}
              />
            </div>
          )}

          <Button type="submit" disabled={pending}>
            {pending ? "…" : cta}
          </Button>
        </form>

        <div className="text-center text-xs text-muted-foreground">
          {mode === "signIn" ? (
            <button
              type="button"
              className="transition-colors hover:text-foreground"
              onClick={() => go("signUp")}
            >
              No account? Create one
            </button>
          ) : (
            <button
              type="button"
              className="transition-colors hover:text-foreground"
              onClick={() => go("signIn")}
            >
              Have an account? Sign in
            </button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
