import { useState } from "react";
import { toast } from "sonner";
import { LogOut, User as UserIcon } from "lucide-react";
import { createTauRPCProxy } from "@/bindings";
import useAuth from "@/hooks/useAuth";
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
 * with the email + sign out. Signed out: a dialog with sign-in / sign-up /
 * email-confirmation. State updates reactively from the `authChanged` event, so
 * a successful sign-in just closes the dialog.
 */
export default function AccountMenu() {
  const status = useAuth();
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<Mode>("signIn");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [pending, setPending] = useState(false);

  // Accounts disabled (or status still loading) — render nothing.
  if (!status?.configured) return null;

  if (status.signedIn) {
    const signOut = () =>
      cmd()
        .sign_out()
        .catch((e) => toast.error(String(e)));
    return (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="outline" size="sm" className="gap-1.5">
            <UserIcon className="size-3.5" />
            <span className="max-w-32 truncate">{status.email ?? "Account"}</span>
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
        </DropdownMenuContent>
      </DropdownMenu>
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
        setOpen(false); // `authChanged` flips the menu to signed-in
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
