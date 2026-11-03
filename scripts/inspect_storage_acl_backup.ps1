<#
Read-only emergency ACL diagnostic for an existing mail-hantong store.

This probe temporarily enables SeBackupPrivilege on its own process token so
that an administrator can inspect an ACL even when the directory DACL denies
READ_CONTROL.  It restores the privilege and closes every handle before
returning.  It never changes ownership/ACLs, creates files, opens database or
key files, follows a reparse point, or prints paths, SIDs, descriptors, or
contents.
#>
& {
$ErrorActionPreference = 'Stop'

if (-not ('HantongStorageAclBackupProbeNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;

public static class HantongStorageAclBackupProbeNative
{
    private const int RPC_E_CHANGED_MODE = unchecked((int)0x80010106);
    private const uint KF_FLAG_NO_PACKAGE_REDIRECTION = 0x00010000;
    private const uint FILE_SHARE_READ = 1, FILE_SHARE_WRITE = 2;
    private const uint OPEN_EXISTING = 3;
    private const uint FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000;
    private const uint FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
    private const uint READ_CONTROL = 0x00020000, FILE_READ_ATTRIBUTES = 0x00000080;
    private const uint TOKEN_QUERY = 0x0008, TOKEN_ADJUST_PRIVILEGES = 0x0020;
    private const uint SE_PRIVILEGE_ENABLED = 0x00000002;
    private const uint TOKEN_PRIVILEGES_CLASS = 3, TOKEN_ELEVATION_CLASS = 20;
    private const uint OWNER_SECURITY_INFORMATION = 1, DACL_SECURITY_INFORMATION = 4;
    private const ushort SE_DACL_PROTECTED = 0x1000;
    private const int SE_FILE_OBJECT = 1;
    private const uint FILE_ATTRIBUTE_REPARSE_POINT = 0x400, FILE_ATTRIBUTE_DIRECTORY = 0x10;
    private const int ERROR_NOT_ALL_ASSIGNED = 1300;

    [StructLayout(LayoutKind.Sequential)] private struct LUID { public uint LowPart; public int HighPart; }
    [StructLayout(LayoutKind.Sequential)] private struct LUID_AND_ATTRIBUTES { public LUID Luid; public uint Attributes; }
    [StructLayout(LayoutKind.Sequential)] private struct TOKEN_PRIVILEGES { public uint Count; public LUID_AND_ATTRIBUTES Entry; }
    [StructLayout(LayoutKind.Sequential)] private struct TOKEN_ELEVATION { public uint TokenIsElevated; }
    [StructLayout(LayoutKind.Sequential)] private struct FILE_ATTRIBUTE_TAG_INFO { public uint FileAttributes; public uint ReparseTag; }

    [DllImport("ole32.dll")] private static extern int CoInitializeEx(IntPtr r, uint f);
    [DllImport("ole32.dll")] private static extern void CoUninitialize();
    [DllImport("ole32.dll")] private static extern void CoTaskMemFree(IntPtr p);
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] private static extern int SHGetKnownFolderPath(ref Guid id, uint flags, IntPtr token, out IntPtr path);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sa, uint disposition, uint flags, IntPtr templateFile);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool CloseHandle(IntPtr h);
    public static void CloseToken(IntPtr h) { if (h != IntPtr.Zero) CloseHandle(h); }
    [DllImport("advapi32.dll", SetLastError = true)] private static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("kernel32.dll")] private static extern IntPtr GetCurrentProcess();
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool LookupPrivilegeValue(string system, string name, ref LUID luid);
    [DllImport("advapi32.dll", SetLastError = true)] private static extern bool AdjustTokenPrivileges(IntPtr token, bool disableAll, ref TOKEN_PRIVILEGES newState, uint length, IntPtr previous, IntPtr returnLength);
    [DllImport("advapi32.dll", SetLastError = true)] private static extern bool GetTokenInformation(IntPtr token, uint klass, IntPtr buffer, uint length, out uint returned);
    [DllImport("advapi32.dll", SetLastError = true)] private static extern uint GetSecurityInfo(IntPtr h, int objectType, uint info, out IntPtr owner, out IntPtr group, out IntPtr dacl, out IntPtr sacl, out IntPtr descriptor);
    [DllImport("advapi32.dll")] private static extern uint GetSecurityDescriptorLength(IntPtr descriptor);
    [DllImport("advapi32.dll")] private static extern bool GetSecurityDescriptorControl(IntPtr descriptor, out ushort control, out uint revision);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetFileInformationByHandleEx(IntPtr h, int klass, out FILE_ATTRIBUTE_TAG_INFO info, uint size);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr p);

    public static int GetLocalAppData(out string path)
    {
        path = null;
        int apartment = CoInitializeEx(IntPtr.Zero, 2);
        if (apartment < 0 && apartment != RPC_E_CHANGED_MODE) return apartment;
        bool uninitialize = apartment >= 0; IntPtr raw = IntPtr.Zero;
        try {
            Guid id = new Guid("F1B32785-6FBA-4FCF-9D55-7B8E7F157091");
            int status = SHGetKnownFolderPath(ref id, KF_FLAG_NO_PACKAGE_REDIRECTION, IntPtr.Zero, out raw);
            if (status != 0 || raw == IntPtr.Zero) return status != 0 ? status : unchecked((int)0x80004005);
            path = Marshal.PtrToStringUni(raw);
            return String.IsNullOrEmpty(path) ? unchecked((int)0x80004005) : 0;
        } finally { if (raw != IntPtr.Zero) CoTaskMemFree(raw); if (uninitialize) CoUninitialize(); }
    }

    public static int EnableBackup(out IntPtr token, out bool elevated, out bool enabled, out bool stateKnown, out int enableError)
    {
        token = IntPtr.Zero; elevated = false; enabled = false; stateKnown = false; enableError = 0;
        if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES, out token)) { enableError = Marshal.GetLastWin32Error(); return 0; }
        uint size = 0; GetTokenInformation(token, TOKEN_ELEVATION_CLASS, IntPtr.Zero, 0, out size);
        IntPtr elevation = Marshal.AllocHGlobal((int)Math.Max(size, 4));
        try { if (GetTokenInformation(token, TOKEN_ELEVATION_CLASS, elevation, size, out size)) elevated = Marshal.ReadInt32(elevation) != 0; } finally { Marshal.FreeHGlobal(elevation); }
        LUID luid = new LUID();
        if (!LookupPrivilegeValue(null, "SeBackupPrivilege", ref luid)) { enableError = Marshal.GetLastWin32Error(); return 0; }
        uint privilegeSize = 0; GetTokenInformation(token, TOKEN_PRIVILEGES_CLASS, IntPtr.Zero, 0, out privilegeSize);
        IntPtr privileges = Marshal.AllocHGlobal((int)Math.Max(privilegeSize, 4));
        uint previous = 0; uint oldAttributes = 0; bool found = false;
        try {
            if (!GetTokenInformation(token, TOKEN_PRIVILEGES_CLASS, privileges, privilegeSize, out previous)) { enableError = Marshal.GetLastWin32Error(); return 0; }
            int count = Marshal.ReadInt32(privileges);
            for (int i = 0; i < count; i++) {
                IntPtr entry = IntPtr.Add(privileges, 4 + i * 12);
                if (Marshal.ReadInt32(entry) == unchecked((int)luid.LowPart) && Marshal.ReadInt32(entry, 4) == luid.HighPart) { oldAttributes = unchecked((uint)Marshal.ReadInt32(entry, 8)); found = true; break; }
            }
            if (!found) { enableError = ERROR_NOT_ALL_ASSIGNED; return 0; }
            stateKnown = true;
            TOKEN_PRIVILEGES state = new TOKEN_PRIVILEGES { Count = 1, Entry = new LUID_AND_ATTRIBUTES { Luid = luid, Attributes = oldAttributes | SE_PRIVILEGE_ENABLED } };
            if (!AdjustTokenPrivileges(token, false, ref state, 0, IntPtr.Zero, IntPtr.Zero)) enableError = Marshal.GetLastWin32Error();
            else { enableError = Marshal.GetLastWin32Error(); enabled = enableError == 0; }
            return unchecked((int)oldAttributes);
        } finally { Marshal.FreeHGlobal(privileges); }
    }

    public static int RestoreBackup(IntPtr token, uint oldAttributes)
    {
        LUID luid = new LUID(); if (!LookupPrivilegeValue(null, "SeBackupPrivilege", ref luid)) return Marshal.GetLastWin32Error();
        TOKEN_PRIVILEGES state = new TOKEN_PRIVILEGES { Count = 1, Entry = new LUID_AND_ATTRIBUTES { Luid = luid, Attributes = oldAttributes } };
        if (!AdjustTokenPrivileges(token, false, ref state, 0, IntPtr.Zero, IntPtr.Zero)) return Marshal.GetLastWin32Error();
        int error = Marshal.GetLastWin32Error(); return error == ERROR_NOT_ALL_ASSIGNED ? error : 0;
    }

    public static int Inspect(string path, out bool isDirectory, out bool reparse, out bool nullDacl, out int securityError, out string ownerClass, out bool protectedDacl,
        out int ruleCount, out uint currentUserAllowMask, out bool hasDeny, out bool hasUnexpectedTrustee)
    {
        isDirectory = false; reparse = false; nullDacl = false; securityError = 0; ownerClass = null; protectedDacl = false; ruleCount = 0; currentUserAllowMask = 0; hasDeny = false; hasUnexpectedTrustee = false;
        IntPtr handle = CreateFileW(path, READ_CONTROL | FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE, IntPtr.Zero, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS, IntPtr.Zero);
        if (handle == IntPtr.Zero || handle.ToInt64() == -1) return Marshal.GetLastWin32Error();
        try {
            FILE_ATTRIBUTE_TAG_INFO tag; if (!GetFileInformationByHandleEx(handle, 9, out tag, (uint)Marshal.SizeOf(typeof(FILE_ATTRIBUTE_TAG_INFO)))) return Marshal.GetLastWin32Error();
            isDirectory = (tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0; reparse = (tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0; if (!isDirectory || reparse) return 0;
            IntPtr owner, group, dacl, sacl, descriptor; uint status = GetSecurityInfo(handle, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, out owner, out group, out dacl, out sacl, out descriptor);
            if (status != 0) { securityError = unchecked((int)status); return 0; }
            try {
                using (WindowsIdentity identity = WindowsIdentity.GetCurrent()) {
                    SecurityIdentifier current = identity.User; SecurityIdentifier system = new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null); SecurityIdentifier admins = new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null);
                    SecurityIdentifier ownerSid = new SecurityIdentifier(owner); ownerClass = ownerSid.Equals(current) ? "current_user" : ownerSid.Equals(system) ? "system" : ownerSid.Equals(admins) ? "administrators" : "other";
                    uint length = GetSecurityDescriptorLength(descriptor); if (length == 0 || length > 1024 * 1024) { securityError = unchecked((int)0x80004005); return 0; }
                    byte[] bytes = new byte[length]; Marshal.Copy(descriptor, bytes, 0, (int)length); RawSecurityDescriptor raw = new RawSecurityDescriptor(bytes, 0); ushort control; uint revision;
                    if (!GetSecurityDescriptorControl(descriptor, out control, out revision)) { securityError = Marshal.GetLastWin32Error(); return 0; }
                    protectedDacl = (control & SE_DACL_PROTECTED) != 0; nullDacl = raw.DiscretionaryAcl == null; if (nullDacl) return 0;
                    foreach (GenericAce generic in raw.DiscretionaryAcl) {
                        ruleCount++; SecurityIdentifier sid = null; int mask = 0; AceQualifier qualifier = AceQualifier.AccessAllowed; CommonAce common = generic as CommonAce; ObjectAce objectAce = generic as ObjectAce;
                        if (common != null) { sid = common.SecurityIdentifier; mask = common.AccessMask; qualifier = common.AceQualifier; } else if (objectAce != null) { sid = objectAce.SecurityIdentifier; mask = objectAce.AccessMask; qualifier = objectAce.AceQualifier; }
                        if (sid == null) continue; if (!(sid.Equals(current) || sid.Equals(system) || sid.Equals(admins))) hasUnexpectedTrustee = true; if (qualifier == AceQualifier.AccessDenied) hasDeny = true; if (sid.Equals(current) && qualifier == AceQualifier.AccessAllowed) currentUserAllowMask |= unchecked((uint)mask);
                    }
                }
                return 0;
            } finally { if (descriptor != IntPtr.Zero) LocalFree(descriptor); }
        } finally { CloseHandle(handle); }
    }
}
'@
}

$localAppData = $null
$knownFolderError = [HantongStorageAclBackupProbeNative]::GetLocalAppData([ref]$localAppData)
if ($knownFolderError -ne 0 -or [string]::IsNullOrEmpty($localAppData)) {
    [pscustomobject]@{ Schema = 'hantong-storage-acl-backup-diagnostic-v1'; KnownFolderError = $knownFolderError; Roots = @() } | ConvertTo-Json -Depth 6 -Compress
    return
}

$token = [IntPtr]::Zero; $elevated = $false; $enabled = $false; $stateKnown = $false; $enableError = 0
$oldAttributes = [HantongStorageAclBackupProbeNative]::EnableBackup([ref]$token, [ref]$elevated, [ref]$enabled, [ref]$stateKnown, [ref]$enableError)
$payload = $null; $restoreError = 0
try {
    $product = Join-Path $localAppData 'mail-hantong'
    $roots = @(
        [pscustomobject]@{ Label = 'product'; Path = $product; ParentGuard = $false },
        [pscustomobject]@{ Label = 'data'; Path = (Join-Path $product 'data'); ParentGuard = $true },
        [pscustomobject]@{ Label = 'secrets'; Path = (Join-Path $product 'secrets'); ParentGuard = $true },
        [pscustomobject]@{ Label = 'profiles'; Path = (Join-Path $product 'profiles'); ParentGuard = $true }
    )
    $reports = @()
    foreach ($root in $roots) {
        $isDirectory = $false; $reparse = $false; $nullDacl = $false; $securityError = 0; $ownerClass = $null; $protectedDacl = $false; $ruleCount = 0; $allowMask = 0; $hasDeny = $false; $hasUnexpected = $false
        if ($root.ParentGuard -and $reports.Count -gt 0 -and ($reports[0].ReadControlOpenError -ne 0 -or $reports[0].ReparsePoint -eq $true -or $reports[0].IsDirectory -ne $true -or $reports[0].GetSecurityInfoError -ne 0)) {
            $reports += [pscustomobject]@{ FixedRoot = $root.Label; BackupPrivilege = $enabled; ParentReparseGuard = $true; ReadControlOpenError = 5; IsDirectory = $null; ReparsePoint = $null; NullDacl = $null; GetSecurityInfoError = $null; OwnerClass = $null; Protected = $null; RuleCount = $null; CurrentUserAllowMask = $null; HasDeny = $null; HasUnexpectedTrustee = $null }
            continue
        }
        $openError = [HantongStorageAclBackupProbeNative]::Inspect($root.Path, [ref]$isDirectory, [ref]$reparse, [ref]$nullDacl, [ref]$securityError, [ref]$ownerClass, [ref]$protectedDacl, [ref]$ruleCount, [ref]$allowMask, [ref]$hasDeny, [ref]$hasUnexpected)
        $reports += [pscustomobject]@{ FixedRoot = $root.Label; BackupPrivilege = $enabled; ParentReparseGuard = $false; ReadControlOpenError = $openError; IsDirectory = if ($openError -eq 0) { $isDirectory } else { $null }; ReparsePoint = if ($openError -eq 0) { $reparse } else { $null }; NullDacl = if ($openError -eq 0 -and $isDirectory -and -not $reparse -and $securityError -eq 0) { $nullDacl } else { $null }; GetSecurityInfoError = if ($openError -eq 0 -and $isDirectory -and -not $reparse) { $securityError } else { $null }; OwnerClass = $ownerClass; Protected = if ($openError -eq 0 -and $isDirectory -and -not $reparse -and $securityError -eq 0) { $protectedDacl } else { $null }; RuleCount = if ($openError -eq 0 -and $isDirectory -and -not $reparse -and $securityError -eq 0) { $ruleCount } else { $null }; CurrentUserAllowMask = if ($openError -eq 0 -and $isDirectory -and -not $reparse -and $securityError -eq 0) { $allowMask } else { $null }; HasDeny = if ($openError -eq 0 -and $isDirectory -and -not $reparse -and $securityError -eq 0) { $hasDeny } else { $null }; HasUnexpectedTrustee = if ($openError -eq 0 -and $isDirectory -and -not $reparse -and $securityError -eq 0) { $hasUnexpected } else { $null } }
    }
    $payload = [pscustomobject]@{ Schema = 'hantong-storage-acl-backup-diagnostic-v1'; KnownFolderError = 0; IsElevated = $elevated; BackupPrivilegeEnableError = $enableError; BackupPrivilegeStateKnown = $stateKnown; BackupPrivilegeEnabled = $enabled; BackupPrivilegeRestoreError = $null; Roots = @($reports) }
} finally {
    if ($token -ne [IntPtr]::Zero) { if ($stateKnown) { $restoreError = [HantongStorageAclBackupProbeNative]::RestoreBackup($token, [uint32]$oldAttributes) }; [HantongStorageAclBackupProbeNative]::CloseToken($token) }
}
if ($null -ne $payload) { $payload.BackupPrivilegeRestoreError = $restoreError; $payload | ConvertTo-Json -Depth 6 -Compress }
}
