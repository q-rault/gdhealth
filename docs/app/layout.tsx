import { RootProvider } from 'fumadocs-ui/provider';
import { Inter } from 'next/font/google';
import type { ReactNode } from 'react';
import './global.css';

const inter = Inter({ subsets: ['latin'] });

export const metadata = {
  title: {
    template: '%s | gdhealth',
    default: 'gdhealth — Code-health analysis for Godot/GDScript',
  },
  description: 'Extract dependency graphs, compute complexity metrics, and enforce architecture policies for Godot projects',
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={inter.className} suppressHydrationWarning>
      <body className="flex min-h-screen flex-col">
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  );
}
