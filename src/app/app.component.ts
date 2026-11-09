import { Component, OnInit, signal, ElementRef, ViewChild } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { invoke } from '@tauri-apps/api/core';

export interface PwaApp {
  id: string;
  name: string;
  url: string;
  icon?: string;
  notifications?: string;
}

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [CommonModule, FormsModule],
  templateUrl: './app.component.html',
  styleUrl: './app.component.css'
})
export class AppComponent implements OnInit {
  apps = signal<PwaApp[]>([]);
  isLoading = signal<boolean>(true);
  isSubmitting = signal<boolean>(false);
  message = signal<string>('');

  // Form model
  appName = '';
  appUrl = '';
  folderId = '';
  isEditing = false;

  @ViewChild('appDialog') appDialog!: ElementRef<HTMLDialogElement>;

  ngOnInit(): void {
    this.loadApps();
  }

  async loadApps(): Promise<void> {
    this.isLoading.set(true);
    try {
      const list = await invoke<PwaApp[]>('get_apps');
      this.apps.set(list);
    } catch (err) {
      console.error('Failed to load apps:', err);
    } finally {
      this.isLoading.set(false);
    }
  }

  dialogError = signal<string>('');

  originalFolderId = '';
  isManualName = false;
  isManualFolder = false;

  isFolderDuplicate(): boolean {
    const clean = (this.folderId || '')
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9-_]/g, '-');
    if (!clean) {
      return false;
    }
    if (this.isEditing && clean === this.originalFolderId.toLowerCase()) {
      return false;
    }
    return this.apps().some(a => a.id.toLowerCase() === clean);
  }

  openCreateModal(): void {
    this.isEditing = false;
    this.originalFolderId = '';
    this.isManualName = false;
    this.isManualFolder = false;
    this.appName = '';
    this.appUrl = '';
    this.folderId = '';
    this.dialogError.set('');
    this.appDialog.nativeElement.showModal();
  }

  openEditModal(app: PwaApp, event: Event): void {
    event.stopPropagation();
    this.isEditing = true;
    this.isManualName = true;
    this.isManualFolder = true;
    this.appName = app.name;
    this.appUrl = app.url;
    this.folderId = app.id;
    this.originalFolderId = app.id;
    this.dialogError.set('');
    this.appDialog.nativeElement.showModal();
  }

  closeModal(): void {
    this.dialogError.set('');
    this.appDialog.nativeElement.close();
  }

  onNameManualInput(): void {
    this.isManualName = true;
  }

  onFolderManualInput(): void {
    this.isManualFolder = true;
    this.dialogError.set('');
  }

  onUrlChange(): void {
    if (this.isEditing) {
      return;
    }

    let raw = (this.appUrl || '').trim();
    if (!raw) {
      if (!this.isManualFolder) this.folderId = '';
      if (!this.isManualName) this.appName = '';
      return;
    }

    if (!raw.startsWith('http://') && !raw.startsWith('https://')) {
      raw = 'https://' + raw;
    }

    try {
      const u = new URL(raw);
      const hostname = u.hostname.replace(/^www\./, '');
      const parts = hostname.split('.');
      const firstPart = parts[0];

      if (firstPart) {
        const cleanId = firstPart.toLowerCase().replace(/[^a-z0-9-_]/g, '-');
        const cleanName = firstPart.charAt(0).toUpperCase() + firstPart.slice(1);

        if (!this.isManualFolder) {
          let candidateId = cleanId;
          let counter = 2;
          while (this.apps().some(a => a.id.toLowerCase() === candidateId.toLowerCase())) {
            candidateId = `${cleanId}-${counter}`;
            counter++;
          }
          this.folderId = candidateId;
        }

        if (!this.isManualName) {
          let candidateName = cleanName;
          let counter = 2;
          while (this.apps().some(a => a.name.toLowerCase() === candidateName.toLowerCase())) {
            candidateName = `${cleanName} (${counter})`;
            counter++;
          }
          this.appName = candidateName;
        }
      }
    } catch {
      // Not a valid URL yet, skip
    }
  }

  async saveApp(): Promise<void> {
    if (!this.appName || !this.appUrl || !this.folderId) {
      return;
    }

    if (this.isFolderDuplicate()) {
      this.dialogError.set('A profile directory with this name already exists. Please choose a different directory name.');
      return;
    }

    this.isSubmitting.set(true);
    this.dialogError.set('');
    try {
      await invoke('save_app', {
        name: this.appName,
        url: this.appUrl,
        folderId: this.folderId,
        isEdit: this.isEditing,
        oldFolderId: this.isEditing ? this.originalFolderId : null
      });
      this.closeModal();
      await this.loadApps();
      this.showTemporaryMessage(`Application "${this.appName}" saved successfully!`);
    } catch (err: unknown) {
      console.error('Failed to save app:', err);
      const errMsg = typeof err === 'string' ? err : (err instanceof Error ? err.message : 'Failed to save application');
      this.dialogError.set(errMsg);
    } finally {
      this.isSubmitting.set(false);
    }
  }

  draggedAppId: string | null = null;
  dragOverAppId: string | null = null;
  private wasDragging = false;

  onDragStart(event: DragEvent, app: PwaApp): void {
    this.draggedAppId = app.id;
    this.wasDragging = true;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', app.id);
    }
  }

  onDragOver(event: DragEvent, app: PwaApp): void {
    if (this.draggedAppId && this.draggedAppId !== app.id) {
      event.preventDefault();
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = 'move';
      }
      this.dragOverAppId = app.id;
    }
  }

  onDragLeave(event: DragEvent, app: PwaApp): void {
    if (this.dragOverAppId === app.id) {
      const related = event.relatedTarget as Node | null;
      const currentTarget = event.currentTarget as Node | null;
      if (!currentTarget || !related || !currentTarget.contains(related)) {
        this.dragOverAppId = null;
      }
    }
  }

  async onDrop(event: DragEvent, targetApp: PwaApp): Promise<void> {
    event.preventDefault();
    const sourceId = this.draggedAppId;
    this.dragOverAppId = null;
    this.draggedAppId = null;

    if (!sourceId || sourceId === targetApp.id) {
      return;
    }

    const currentApps = [...this.apps()];
    const fromIndex = currentApps.findIndex(a => a.id === sourceId);
    const toIndex = currentApps.findIndex(a => a.id === targetApp.id);

    if (fromIndex !== -1 && toIndex !== -1) {
      const [moved] = currentApps.splice(fromIndex, 1);
      currentApps.splice(toIndex, 0, moved);
      this.apps.set(currentApps);
      try {
        await invoke('reorder_apps', { appIds: currentApps.map(a => a.id) });
      } catch (err) {
        console.error('Failed to save reordered apps:', err);
      }
    }
  }

  onDragEnd(): void {
    this.draggedAppId = null;
    this.dragOverAppId = null;
    setTimeout(() => {
      this.wasDragging = false;
    }, 150);
  }

  async launchApp(app: PwaApp): Promise<void> {
    if (this.wasDragging) {
      return;
    }
    try {
      await invoke('launch_app', { id: app.id });
    } catch (err) {
      console.error('Failed to launch app:', err);
      alert(`Error launching app: ${err}`);
    }
  }

  async createShortcut(app: PwaApp, event: Event): Promise<void> {
    event.stopPropagation();
    try {
      const path = await invoke<string>('create_desktop_shortcut', { id: app.id });
      this.showTemporaryMessage(`Shortcut created at ${path}`);
    } catch (err) {
      console.error('Failed to create shortcut:', err);
      alert(`Error creating shortcut: ${err}`);
    }
  }

  async deleteApp(app: PwaApp, event: Event): Promise<void> {
    event.stopPropagation();
    if (confirm(`Are you sure you want to delete "${app.name}"?`)) {
      try {
        await invoke('delete_app', { id: app.id });
        await this.loadApps();
        this.showTemporaryMessage(`Application "${app.name}" removed.`);
      } catch (err) {
        console.error('Failed to delete app:', err);
        alert(`Error deleting app: ${err}`);
      }
    }
  }

  selectedAppForPerms = signal<PwaApp | null>(null);
  @ViewChild('permissionsDialog') permissionsDialog!: ElementRef<HTMLDialogElement>;

  getPermissionLabel(status?: string): string {
    if (status === 'granted') {
      return 'Allowed';
    }
    if (status === 'denied') {
      return 'Blocked';
    }
    return 'Ask';
  }

  openPermissionsModal(app: PwaApp, event: Event): void {
    event.stopPropagation();
    this.selectedAppForPerms.set(app);
    this.permissionsDialog.nativeElement.showModal();
  }

  closePermissionsModal(): void {
    this.selectedAppForPerms.set(null);
    this.permissionsDialog.nativeElement.close();
  }

  async setAppPermission(app: PwaApp, permission: string): Promise<void> {
    try {
      await invoke('set_app_notification_permission', { id: app.id, permission });
      const permVal = permission === 'default' ? undefined : permission;
      const currentList = this.apps().map(a => {
        if (a.id === app.id) {
          return {
            ...a,
            notifications: permVal
          };
        }
        return a;
      });
      this.apps.set(currentList);
      if (this.selectedAppForPerms()?.id === app.id) {
        this.selectedAppForPerms.set({
          ...app,
          notifications: permVal
        });
      }
      this.showTemporaryMessage(`Notification permission for "${app.name}" updated to ${this.getPermissionLabel(permVal)}.`);
    } catch (err) {
      console.error('Failed to update notification permission:', err);
      alert(`Error updating notification permission: ${err}`);
    }
  }

  private showTemporaryMessage(msg: string): void {
    this.message.set(msg);
    setTimeout(() => {
      this.message.set('');
    }, 4000);
  }
}
