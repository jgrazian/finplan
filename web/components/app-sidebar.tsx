"use client"

import * as React from "react"
import {
  BarChart3,
  Calculator,
  FileText,
  History,
  Home,
  LineChart,
  PiggyBank,
  Plus,
  Settings2,
  TrendingUp,
  Wallet,
} from "lucide-react"

import { NavMain } from "@/components/nav-main"
import { NavProjects } from "@/components/nav-projects"
import { NavUser } from "@/components/nav-user"
import { TeamSwitcher } from "@/components/team-switcher"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarRail,
} from "@/components/ui/sidebar"

// Application navigation data
const data = {
  user: {
    name: "User",
    email: "user@finplan.app",
    avatar: "",
  },
  teams: [
    {
      name: "FinPlan",
      logo: TrendingUp,
      plan: "Personal",
    },
  ],
  navMain: [
    {
      title: "Dashboard",
      url: "/",
      icon: Home,
      isActive: true,
    },
    {
      title: "Portfolios",
      url: "/portfolios",
      icon: Wallet,
      items: [
        {
          title: "All Portfolios",
          url: "/portfolios",
        },
        {
          title: "New Portfolio",
          url: "/portfolios/new",
        },
      ],
    },
    {
      title: "Simulations",
      url: "/simulations",
      icon: Calculator,
      items: [
        {
          title: "All Simulations",
          url: "/simulations",
        },
        {
          title: "New Simulation",
          url: "/simulations/new",
        },
      ],
    },
    {
      title: "Results",
      url: "/results",
      icon: BarChart3,
      items: [
        {
          title: "Portfolio Projections",
          url: "/results",
        },
        {
          title: "Run History",
          url: "/results/history",
        },
      ],
    },
  ],
  projects: [
    {
      name: "New Portfolio",
      url: "/portfolios/new",
      icon: Wallet,
    },
    {
      name: "New Simulation",
      url: "/simulations/new",
      icon: Plus,
    },
  ],
}

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  return (
    <Sidebar collapsible="icon" {...props}>
      <SidebarHeader>
        <TeamSwitcher teams={data.teams} />
      </SidebarHeader>
      <SidebarContent>
        <NavMain items={data.navMain} />
        <NavProjects projects={data.projects} />
      </SidebarContent>
      <SidebarFooter>
        <NavUser user={data.user} />
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  )
}
