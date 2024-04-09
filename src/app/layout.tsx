import { Toaster } from "@/components/ui/sonner";
import type { Metadata } from "next";
import { Inter } from "next/font/google";
import "@/app/globals.css";

const inter = Inter({ subsets: ["latin"] });

export const metadata: Metadata = {
  title: "Lux.app",
  description: "Built with Tauri and Next.js",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body className={`${inter.className} dark`}>
        <div className="min-h-screen flex flex-col justify-center sm:px-12">
          {children}
        </div>
        <Toaster closeButton={true} />
      </body>
    </html>
  );
}
