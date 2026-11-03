<#
Read-only Windows storage-startup probe.

This script intentionally does not create, modify, delete, or read application
files.  It asks the OS for LocalAppData, opens only the fixed directory roots
with attribute/traverse access, and reports bounded Win32 diagnostics without
printing paths, usernames, or file identifiers.
#>

& {
$ErrorActionPreference = 'Stop'

if (-not ('HantongStorageProbeNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class HantongStorageProbeNative
{
    private const uint KF_FLAG_NO_PACKAGE_REDIRECTION = 0x00010000;
    private const int RPC_E_CHANGED_MODE = unchecked((int)0x80010106);
    private const int S_OK = 0;
    private const uint FILE_SHARE_READ = 0x00000001;
    private const uint FILE_SHARE_WRITE = 0x00000002;
    private const uint OPEN_EXISTING = 3;
    private const uint FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000;
    private const uint FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
    private const uint INVALID_HANDLE_VALUE = unchecked((uint)0xFFFFFFFF);
    private const int FileIdInfo = 18;
    private const int FileAttributeTagInfo = 9;

    [DllImport("ole32.dll")]
    private static extern int CoInitializeEx(IntPtr reserved, uint coInit);

    [DllImport("ole32.dll")]
    private static extern void CoUninitialize();

    [DllImport("ole32.dll")]
    private static extern void CoTaskMemFree(IntPtr memory);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHGetKnownFolderPath(
        ref Guid rfid, uint flags, IntPtr token, out IntPtr path);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateFileW(
        string name, uint access, uint share, IntPtr security,
        uint disposition, uint flags, IntPtr templateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetFileInformationByHandleEx(
        IntPtr handle, int infoClass, IntPtr info, uint size);

    public static int GetLocalAppData(out string path)
    {
        path = null;
        int apartment = CoInitializeEx(IntPtr.Zero, 2); // COINIT_APARTMENTTHREADED
        if (apartment < 0 && apartment != RPC_E_CHANGED_MODE)
            return apartment;
        bool uninitialize = apartment >= 0;
        IntPtr raw = IntPtr.Zero;
        try
        {
            Guid id = new Guid("F1B32785-6FBA-4FCF-9D55-7B8E7F157091");
            int status = SHGetKnownFolderPath(ref id, KF_FLAG_NO_PACKAGE_REDIRECTION,
                IntPtr.Zero, out raw);
            if (status != S_OK || raw == IntPtr.Zero)
                return status;
            path = Marshal.PtrToStringUni(raw);
            return String.IsNullOrEmpty(path) ? unchecked((int)0x80004005) : S_OK;
        }
        finally
        {
            if (raw != IntPtr.Zero)
                CoTaskMemFree(raw);
            if (uninitialize)
                CoUninitialize();
        }
    }

    public static int OpenDirectory(string path, uint access, out IntPtr handle)
    {
        handle = CreateFileW(path, access, FILE_SHARE_READ | FILE_SHARE_WRITE,
            IntPtr.Zero, OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            IntPtr.Zero);
        if (handle == IntPtr.Zero || handle.ToInt64() == -1)
        {
            handle = IntPtr.Zero;
            return Marshal.GetLastWin32Error();
        }
        return 0;
    }

    public static void Close(IntPtr handle)
    {
        if (handle != IntPtr.Zero)
            CloseHandle(handle);
    }

    // Returns zero on success, otherwise the Win32 error.  The 24-byte result
    // is deliberately consumed here and never returned by this diagnostic.
    public static int FileIdInfoError(IntPtr handle)
    {
        IntPtr buffer = Marshal.AllocHGlobal(24);
        try
        {
            if (GetFileInformationByHandleEx(handle, FileIdInfo, buffer, 24))
                return 0;
            return Marshal.GetLastWin32Error();
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    public static int ReparseInfo(IntPtr handle, out bool reparse)
    {
        reparse = false;
        IntPtr buffer = Marshal.AllocHGlobal(8);
        try
        {
            if (!GetFileInformationByHandleEx(handle, FileAttributeTagInfo, buffer, 8))
                return Marshal.GetLastWin32Error();
            uint attributes = unchecked((uint)Marshal.ReadInt32(buffer));
            reparse = (attributes & 0x00000400) != 0; // FILE_ATTRIBUTE_REPARSE_POINT
            return 0;
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }
}
'@
}

$localAppData = $null
$knownFolderError = [HantongStorageProbeNative]::GetLocalAppData([ref]$localAppData)
if ($knownFolderError -ne 0 -or [string]::IsNullOrEmpty($localAppData)) {
    [pscustomobject]@{
        Schema = 'hantong-storage-diagnostic-v1'
        KnownFolderError = $knownFolderError
        Roots = @()
    } | ConvertTo-Json -Depth 5 -Compress
    return
}

$product = Join-Path $localAppData 'mail-hantong'
$fixedRoots = @(
    [pscustomobject]@{ Label = 'product'; Path = $product },
    [pscustomobject]@{ Label = 'data'; Path = (Join-Path $product 'data') },
    [pscustomobject]@{ Label = 'secrets'; Path = (Join-Path $product 'secrets') },
    [pscustomobject]@{ Label = 'profiles'; Path = (Join-Path $product 'profiles') }
)

$results = foreach ($root in $fixedRoots) {
    # Build every ancestor from the volume root (for example C:\) through the
    # fixed root.  Paths are retained only as handles' input and never emitted.
    $full = $root.Path.TrimEnd('\')
    $volume = [IO.Path]::GetPathRoot($full)
    $rest = $full.Substring($volume.Length).Trim('\')
    $ancestors = @($volume)
    if ($rest.Length -gt 0) {
        $built = $volume
        foreach ($part in $rest.Split('\')) {
            $built = Join-Path $built $part
            $ancestors += $built
        }
    }
    $failures = @()
    $stopped = $false
    for ($index = 0; $index -lt $ancestors.Count -and -not $stopped; $index++) {
        $isLeaf = ($index -eq ($ancestors.Count - 1))
        $probes = @(
            [pscustomobject]@{ Name = 'READ_ATTRIBUTES|LIST_DIRECTORY'; Access = [uint32](0x00000080 -bor 0x00000001) },
            [pscustomobject]@{ Name = 'READ_ATTRIBUTES|TRAVERSE'; Access = [uint32](0x00000080 -bor 0x00000020) }
        )
        $reparseSeen = $false
        foreach ($probe in $probes) {
            $handle = [IntPtr]::Zero
            $openError = [HantongStorageProbeNative]::OpenDirectory($ancestors[$index], $probe.Access, [ref]$handle)
            $fileIdError = $null
            $reparseError = $null
            $reparse = $null
            try {
                if ($openError -eq 0) {
                    $fileIdError = [HantongStorageProbeNative]::FileIdInfoError($handle)
                    $reparseValue = $false
                    $reparseError = [HantongStorageProbeNative]::ReparseInfo($handle, [ref]$reparseValue)
                    if ($reparseError -eq 0) {
                        $reparse = $reparseValue
                        if ($reparse) { $reparseSeen = $true }
                    }
                }
            } finally {
                [HantongStorageProbeNative]::Close($handle)
            }
            if ($openError -ne 0 -or $fileIdError -ne 0 -or $reparseError -ne 0 -or $reparse -eq $true) {
                $failures += [pscustomobject]@{
                    AncestorIndex = $index
                    Access = $probe.Name
                    Win32OpenError = $openError
                    FileIdInfoBytes = 24
                    FileIdInfoError = $fileIdError
                    ReparseInfoError = $reparseError
                    ReparsePoint = $reparse
                }
            }
        }
        if ($isLeaf -and ($root.Label -eq 'data' -or $root.Label -eq 'secrets')) {
            # OPEN_EXISTING only: FILE_ADD_FILE checks the effective ACL needed
            # for a future database/lock file without creating one.
            $addFileHandle = [IntPtr]::Zero
            try {
                $addFileOpenError = [HantongStorageProbeNative]::OpenDirectory(
                    $ancestors[$index], [uint32](0x00000080 -bor 0x00000002), [ref]$addFileHandle)
            } finally {
                [HantongStorageProbeNative]::Close($addFileHandle)
            }
        } else {
            $addFileOpenError = $null
        }
        if ($reparseSeen) { $stopped = $true }
    }

    [pscustomobject]@{
        FixedRoot = $root.Label
        AncestorCount = $ancestors.Count
        Failures = @($failures)
        AddFileOpenError = $addFileOpenError
    }
}

[pscustomobject]@{
    Schema = 'hantong-storage-diagnostic-v1'
    KnownFolderError = 0
    Roots = @($results)
} | ConvertTo-Json -Depth 8 -Compress
}
