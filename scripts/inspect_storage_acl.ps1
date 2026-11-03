<#
Read-only ACL diagnostic for the fixed mail-hantong directories.
It opens directories with READ_CONTROL and reads only owner/DACL metadata.  It
never changes ACLs, creates files, opens database/key files, or prints paths,
SIDs, usernames, descriptors, or file contents.
#>
& {
$ErrorActionPreference = 'Stop'

if (-not ('HantongStorageAclProbeNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Runtime.InteropServices;

public static class HantongStorageAclProbeNative
{
    private const int RPC_E_CHANGED_MODE = unchecked((int)0x80010106);
    private const uint KF_FLAG_NO_PACKAGE_REDIRECTION = 0x00010000;
    private const uint FILE_SHARE_READ = 1;
    private const uint FILE_SHARE_WRITE = 2;
    private const uint OPEN_EXISTING = 3;
    private const uint FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000;
    private const uint FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
    private const uint READ_CONTROL = 0x00020000;
    private const int SE_FILE_OBJECT = 1;
    private const uint OWNER_SECURITY_INFORMATION = 1;
    private const uint DACL_SECURITY_INFORMATION = 4;
    private const ushort SE_DACL_PROTECTED = 0x1000;

    [DllImport("ole32.dll")] private static extern int CoInitializeEx(IntPtr r, uint f);
    [DllImport("ole32.dll")] private static extern void CoUninitialize();
    [DllImport("ole32.dll")] private static extern void CoTaskMemFree(IntPtr p);
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHGetKnownFolderPath(ref Guid id, uint flags, IntPtr token, out IntPtr path);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sa, uint disposition, uint flags, IntPtr templateFile);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern uint GetSecurityInfo(IntPtr h, int objectType, uint info,
        out IntPtr owner, out IntPtr group, out IntPtr dacl, out IntPtr sacl, out IntPtr descriptor);
    [DllImport("advapi32.dll")] private static extern uint GetSecurityDescriptorLength(IntPtr descriptor);
    [DllImport("advapi32.dll")] private static extern bool GetSecurityDescriptorControl(IntPtr descriptor, out ushort control, out uint revision);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr p);

    public static int GetLocalAppData(out string path)
    {
        path = null;
        int apartment = CoInitializeEx(IntPtr.Zero, 2);
        if (apartment < 0 && apartment != RPC_E_CHANGED_MODE) return apartment;
        bool uninitialize = apartment >= 0;
        IntPtr raw = IntPtr.Zero;
        try
        {
            Guid id = new Guid("F1B32785-6FBA-4FCF-9D55-7B8E7F157091");
            int status = SHGetKnownFolderPath(ref id, KF_FLAG_NO_PACKAGE_REDIRECTION, IntPtr.Zero, out raw);
            if (status != 0) return status;
            if (raw == IntPtr.Zero) return unchecked((int)0x80004005);
            path = Marshal.PtrToStringUni(raw);
            return String.IsNullOrEmpty(path) ? unchecked((int)0x80004005) : 0;
        }
        finally
        {
            if (raw != IntPtr.Zero) CoTaskMemFree(raw);
            if (uninitialize) CoUninitialize();
        }
    }

    public static int Inspect(string path, out int securityError, out string ownerClass, out bool protectedDacl,
        out int ruleCount, out uint currentUserAllowMask, out bool hasDeny,
        out bool hasUnexpectedTrustee)
    {
        securityError = 0;
        ownerClass = null;
        protectedDacl = false;
        ruleCount = 0;
        currentUserAllowMask = 0;
        hasDeny = false;
        hasUnexpectedTrustee = false;
        IntPtr handle = CreateFileW(path, READ_CONTROL, FILE_SHARE_READ | FILE_SHARE_WRITE,
            IntPtr.Zero, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS, IntPtr.Zero);
        if (handle == IntPtr.Zero || handle.ToInt64() == -1) return Marshal.GetLastWin32Error();
        try
        {
            IntPtr owner, group, dacl, sacl, descriptor;
            uint status = GetSecurityInfo(handle, SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                out owner, out group, out dacl, out sacl, out descriptor);
            if (status != 0) { if (descriptor != IntPtr.Zero) LocalFree(descriptor); securityError = unchecked((int)status); return 0; }
            try
            {
                using (WindowsIdentity identity = WindowsIdentity.GetCurrent()) {
                SecurityIdentifier current = identity.User;
                SecurityIdentifier system = new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null);
                SecurityIdentifier admins = new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null);
                SecurityIdentifier ownerSid = new SecurityIdentifier(owner);
                ownerClass = ownerSid.Equals(current) ? "current_user" :
                    ownerSid.Equals(system) ? "system" :
                    ownerSid.Equals(admins) ? "administrators" : "other";

                uint length = GetSecurityDescriptorLength(descriptor);
                if (length == 0 || length > 1024 * 1024) { securityError = unchecked((int)0x80004005); return 0; }
                byte[] bytes = new byte[length];
                Marshal.Copy(descriptor, bytes, 0, (int)length);
                RawSecurityDescriptor raw = new RawSecurityDescriptor(bytes, 0);
                ushort control;
                uint revision;
                if (!GetSecurityDescriptorControl(descriptor, out control, out revision))
                {
                    securityError = Marshal.GetLastWin32Error();
                    return 0;
                }
                protectedDacl = (control & SE_DACL_PROTECTED) != 0;
                if (raw.DiscretionaryAcl == null) return 0;
                foreach (GenericAce generic in raw.DiscretionaryAcl)
                {
                    ruleCount++;
                    SecurityIdentifier sid = null;
                    int mask = 0;
                    AceQualifier qualifier = AceQualifier.AccessAllowed;
                    CommonAce common = generic as CommonAce;
                    ObjectAce objectAce = generic as ObjectAce;
                    if (common != null) { sid = common.SecurityIdentifier; mask = common.AccessMask; qualifier = common.AceQualifier; }
                    else if (objectAce != null) { sid = objectAce.SecurityIdentifier; mask = objectAce.AccessMask; qualifier = objectAce.AceQualifier; }
                    if (sid == null) continue;
                    bool expected = sid.Equals(current) || sid.Equals(system) || sid.Equals(admins);
                    if (!expected) hasUnexpectedTrustee = true;
                    if (qualifier == AceQualifier.AccessDenied) hasDeny = true;
                    if (sid.Equals(current) && qualifier == AceQualifier.AccessAllowed)
                        currentUserAllowMask |= unchecked((uint)mask);
                }
                return 0;
                }
            }
            finally
            {
                if (descriptor != IntPtr.Zero) LocalFree(descriptor);
            }
        }
        finally
        {
            CloseHandle(handle);
        }
    }
}
'@
}

$localAppData = $null
$knownFolderError = [HantongStorageAclProbeNative]::GetLocalAppData([ref]$localAppData)
if ($knownFolderError -ne 0 -or [string]::IsNullOrEmpty($localAppData)) {
    [pscustomobject]@{ Schema = 'hantong-storage-acl-diagnostic-v1'; KnownFolderError = $knownFolderError; Roots = @() } |
        ConvertTo-Json -Depth 6 -Compress
    return
}

$product = Join-Path $localAppData 'mail-hantong'
$roots = @(
    [pscustomobject]@{ Label = 'product'; Path = $product },
    [pscustomobject]@{ Label = 'data'; Path = (Join-Path $product 'data') },
    [pscustomobject]@{ Label = 'secrets'; Path = (Join-Path $product 'secrets') },
    [pscustomobject]@{ Label = 'profiles'; Path = (Join-Path $product 'profiles') }
)

$reports = foreach ($root in $roots) {
    $securityError = 0; $ownerClass = $null; $protectedDacl = $false; $ruleCount = 0; $allowMask = 0
    $hasDeny = $false; $hasUnexpected = $false
    $readControlError = [HantongStorageAclProbeNative]::Inspect($root.Path, [ref]$securityError, [ref]$ownerClass,
        [ref]$protectedDacl, [ref]$ruleCount, [ref]$allowMask, [ref]$hasDeny, [ref]$hasUnexpected)
    [pscustomobject]@{
        FixedRoot = $root.Label
        ReadControlOpenError = $readControlError
        GetSecurityInfoError = if ($readControlError -eq 0) { $securityError } else { $null }
        OwnerClass = $ownerClass
        Protected = if ($readControlError -eq 0 -and $securityError -eq 0) { $protectedDacl } else { $null }
        RuleCount = if ($readControlError -eq 0 -and $securityError -eq 0) { $ruleCount } else { $null }
        CurrentUserAllowMask = if ($readControlError -eq 0 -and $securityError -eq 0) { $allowMask } else { $null }
        HasDeny = if ($readControlError -eq 0 -and $securityError -eq 0) { $hasDeny } else { $null }
        HasUnexpectedTrustee = if ($readControlError -eq 0 -and $securityError -eq 0) { $hasUnexpected } else { $null }
    }
}

[pscustomobject]@{
    Schema = 'hantong-storage-acl-diagnostic-v1'
    KnownFolderError = 0
    Roots = @($reports)
} | ConvertTo-Json -Depth 6 -Compress
}
