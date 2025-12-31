"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { formatDistanceToNow } from "date-fns";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { MoreHorizontal, Play, Pencil, Trash2, Plus, FileText } from "lucide-react";
import { SimulationListItem } from "@/lib/types";
import { listSimulations, deleteSimulation } from "@/lib/api";

export function SimulationsList() {
    const router = useRouter();
    const [simulations, setSimulations] = React.useState<SimulationListItem[]>([]);
    const [isLoading, setIsLoading] = React.useState(true);
    const [error, setError] = React.useState<string | null>(null);
    const [deleteId, setDeleteId] = React.useState<string | null>(null);
    const [isDeleting, setIsDeleting] = React.useState(false);

    const fetchSimulations = React.useCallback(async () => {
        try {
            setIsLoading(true);
            const data = await listSimulations();
            setSimulations(data);
            setError(null);
        } catch (err) {
            setError("Failed to load simulations. Make sure the API server is running.");
            console.error(err);
        } finally {
            setIsLoading(false);
        }
    }, []);

    React.useEffect(() => {
        fetchSimulations();
    }, [fetchSimulations]);

    const handleDelete = async () => {
        if (!deleteId) return;
        setIsDeleting(true);
        try {
            await deleteSimulation(deleteId);
            setSimulations((prev) => prev.filter((s) => s.id !== deleteId));
            setDeleteId(null);
        } catch (err) {
            console.error("Failed to delete simulation:", err);
        } finally {
            setIsDeleting(false);
        }
    };

    if (isLoading) {
        return (
            <Card>
                <CardContent className="flex items-center justify-center py-10">
                    <div className="text-muted-foreground">Loading simulations...</div>
                </CardContent>
            </Card>
        );
    }

    if (error) {
        return (
            <Card>
                <CardContent className="flex flex-col items-center justify-center py-10">
                    <p className="text-destructive mb-4">{error}</p>
                    <Button onClick={fetchSimulations} variant="outline">
                        Retry
                    </Button>
                </CardContent>
            </Card>
        );
    }

    if (simulations.length === 0) {
        return (
            <Card>
                <CardContent className="flex flex-col items-center justify-center py-16">
                    <FileText className="h-12 w-12 text-muted-foreground mb-4" />
                    <h3 className="text-lg font-semibold mb-2">No simulations yet</h3>
                    <p className="text-muted-foreground text-center mb-6 max-w-md">
                        Create your first financial simulation to start planning your future.
                    </p>
                    <Button asChild>
                        <Link href="/simulations/new">
                            <Plus className="mr-2 h-4 w-4" />
                            Create Simulation
                        </Link>
                    </Button>
                </CardContent>
            </Card>
        );
    }

    return (
        <>
            <Card>
                <CardHeader>
                    <div className="flex items-center justify-between">
                        <div>
                            <CardTitle>Your Simulations</CardTitle>
                            <CardDescription>
                                Manage and run your financial planning scenarios
                            </CardDescription>
                        </div>
                        <Button asChild>
                            <Link href="/simulations/new">
                                <Plus className="mr-2 h-4 w-4" />
                                New Simulation
                            </Link>
                        </Button>
                    </div>
                </CardHeader>
                <CardContent>
                    <Table>
                        <TableHeader>
                            <TableRow>
                                <TableHead>Name</TableHead>
                                <TableHead>Description</TableHead>
                                <TableHead>Created</TableHead>
                                <TableHead>Last Updated</TableHead>
                                <TableHead className="w-[70px]"></TableHead>
                            </TableRow>
                        </TableHeader>
                        <TableBody>
                            {simulations.map((sim) => (
                                <TableRow key={sim.id}>
                                    <TableCell>
                                        <Link
                                            href={`/simulations/${sim.id}`}
                                            className="font-medium hover:underline"
                                        >
                                            {sim.name}
                                        </Link>
                                    </TableCell>
                                    <TableCell className="text-muted-foreground max-w-[300px] truncate">
                                        {sim.description || "—"}
                                    </TableCell>
                                    <TableCell className="text-muted-foreground">
                                        {formatDistanceToNow(new Date(sim.created_at), { addSuffix: true })}
                                    </TableCell>
                                    <TableCell className="text-muted-foreground">
                                        {formatDistanceToNow(new Date(sim.updated_at), { addSuffix: true })}
                                    </TableCell>
                                    <TableCell>
                                        <DropdownMenu>
                                            <DropdownMenuTrigger asChild>
                                                <Button variant="ghost" size="icon">
                                                    <MoreHorizontal className="h-4 w-4" />
                                                    <span className="sr-only">Actions</span>
                                                </Button>
                                            </DropdownMenuTrigger>
                                            <DropdownMenuContent align="end">
                                                <DropdownMenuItem
                                                    onClick={() => router.push(`/simulations/${sim.id}`)}
                                                >
                                                    <Play className="mr-2 h-4 w-4" />
                                                    View & Run
                                                </DropdownMenuItem>
                                                <DropdownMenuItem
                                                    onClick={() => router.push(`/simulations/${sim.id}/edit`)}
                                                >
                                                    <Pencil className="mr-2 h-4 w-4" />
                                                    Edit
                                                </DropdownMenuItem>
                                                <DropdownMenuSeparator />
                                                <DropdownMenuItem
                                                    className="text-destructive"
                                                    onClick={() => setDeleteId(sim.id)}
                                                >
                                                    <Trash2 className="mr-2 h-4 w-4" />
                                                    Delete
                                                </DropdownMenuItem>
                                            </DropdownMenuContent>
                                        </DropdownMenu>
                                    </TableCell>
                                </TableRow>
                            ))}
                        </TableBody>
                    </Table>
                </CardContent>
            </Card>

            {/* Delete Confirmation Dialog */}
            <Dialog open={!!deleteId} onOpenChange={() => setDeleteId(null)}>
                <DialogContent>
                    <DialogHeader>
                        <DialogTitle>Delete Simulation</DialogTitle>
                        <DialogDescription>
                            Are you sure you want to delete this simulation? This action cannot be undone.
                        </DialogDescription>
                    </DialogHeader>
                    <DialogFooter>
                        <Button variant="outline" onClick={() => setDeleteId(null)}>
                            Cancel
                        </Button>
                        <Button
                            variant="destructive"
                            onClick={handleDelete}
                            disabled={isDeleting}
                        >
                            {isDeleting ? "Deleting..." : "Delete"}
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>
        </>
    );
}
