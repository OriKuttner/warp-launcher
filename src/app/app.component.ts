import { Component, OnInit, signal, ElementRef, ViewChild } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { invoke } from '@tauri-apps/api/core';

export interface PwaApp {
  id: string;
  name: string;
  url: string;
  icon?: string;
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

  isManualName = false;
  isManualFolder = false;

  openCreateModal(): void {
    this.isEditing = false;
    this.isManualName = false;
    this.isManualFolder = false;
    this.appName = '';
    this.appUrl = '';
    this.folderId = '';
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
    this.appDialog.nativeElement.showModal();
  }

  closeModal(): void {
    this.appDialog.nativeElement.close();
  }

  onNameManualInput(): void {
    this.isManualName = true;
  }

  onFolderManualInput(): void {
    this.isManualFolder = true;
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
        const cleanId = firstPart.toLowerCase().replace(/[^a-z0-9-]/g, '-');
        const cleanName = firstPart.charAt(0).toUpperCase() + firstPart.slice(1);

        if (!this.isManualFolder) {
          this.folderId = cleanId;
        }
        if (!this.isManualName) {
          this.appName = cleanName;
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

    this.isSubmitting.set(true);
    try {
      await invoke('save_app', {
        name: this.appName,
        url: this.appUrl,
        folderId: this.folderId
      });
      this.closeModal();
      await this.loadApps();
      this.showTemporaryMessage(`Application "${this.appName}" saved successfully!`);
    } catch (err) {
      console.error('Failed to save app:', err);
      alert(`Error saving app: ${err}`);
    } finally {
      this.isSubmitting.set(false);
    }
  }

  async launchApp(app: PwaApp): Promise<void> {
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

  private showTemporaryMessage(msg: string): void {
    this.message.set(msg);
    setTimeout(() => {
      this.message.set('');
    }, 4000);
  }
}
