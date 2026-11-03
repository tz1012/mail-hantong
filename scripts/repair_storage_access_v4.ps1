<# Explicit recovery of the previously approved nine existing objects. Dry-run by default. Apply adds the current user ACE only after validating the protected original backup, object identities and original owner/DACL; it never reads or writes database/key contents. #>
param([switch]$Apply,[switch]$ConfirmRepair,[string]$ExpectedPlanHash)
& {
$ErrorActionPreference='Stop'
if(-not ('HantongStorageRepairV4Native' -as [type])){Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;

public static class HantongStorageRepairV4Native
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

    public static int EnableBackup(string privilegeName,out IntPtr token, out bool elevated, out bool enabled, out bool stateKnown, out int enableError)
    {
        token = IntPtr.Zero; elevated = false; enabled = false; stateKnown = false; enableError = 0;
        if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES, out token)) { enableError = Marshal.GetLastWin32Error(); return 0; }
        uint size = 0; GetTokenInformation(token, TOKEN_ELEVATION_CLASS, IntPtr.Zero, 0, out size);
        IntPtr elevation = Marshal.AllocHGlobal((int)Math.Max(size, 4));
        try { if (GetTokenInformation(token, TOKEN_ELEVATION_CLASS, elevation, size, out size)) elevated = Marshal.ReadInt32(elevation) != 0; } finally { Marshal.FreeHGlobal(elevation); }
        LUID luid = new LUID();
        if (!LookupPrivilegeValue(null, privilegeName, ref luid)) { enableError = Marshal.GetLastWin32Error(); return 0; }
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

    public static int RestoreBackup(string privilegeName,IntPtr token, uint oldAttributes)
    {
        LUID luid = new LUID(); if (!LookupPrivilegeValue(null, privilegeName, ref luid)) return Marshal.GetLastWin32Error();
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
    [StructLayout(LayoutKind.Sequential)] private struct TreeInfo {
        public uint Attributes; public System.Runtime.InteropServices.ComTypes.FILETIME Creation,Access,Write;
        public uint Volume,SizeHigh,SizeLow,Links,IndexHigh,IndexLow;
    }
    [StructLayout(LayoutKind.Sequential)] private struct IoStatus { public IntPtr Status; public UIntPtr Information; }
    [DllImport("kernel32.dll",SetLastError=true)] private static extern bool GetFileInformationByHandle(IntPtr h,out TreeInfo info);
    [DllImport("ntdll.dll")] private static extern int NtQueryDirectoryFile(IntPtr h,IntPtr e,IntPtr apc,IntPtr context,ref IoStatus status,IntPtr buffer,uint size,int klass,[MarshalAs(UnmanagedType.U1)] bool single,IntPtr name,[MarshalAs(UnmanagedType.U1)] bool restart);
    public sealed class TreeReport {
        public string ErrorCode="OK", PlanHash,FailureCause; public int ChangedBeforeRollback;
        public bool Applied,BackupSaved,RollbackAttempted,RollbackComplete; public int NodesChanged; public string RecoveryId; public int NodesScanned,DirectoriesScanned,FilesScanned,CurrentUserOwners,AdminOwners,SystemOwners,OtherOwners,DenyNodes,UnknownTrusteeNodes,UnsupportedAceNodes,MissingUserFullControlNodes,LogonReadOnlyNodes,LogonBroadNodes;
    }
    private sealed class TreeEntry { public string Path,Relative; public int Depth; public bool Directory; }
    private static IntPtr TreeOpen(string path,bool directory,bool writeDacl=false) {
        uint access=READ_CONTROL|FILE_READ_ATTRIBUTES|0x00100000U|(directory?1U:0U)|(writeDacl?0x40000U:0U);
        IntPtr h=CreateFileW(path,access,FILE_SHARE_READ|FILE_SHARE_WRITE,IntPtr.Zero,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_BACKUP_SEMANTICS,IntPtr.Zero);
        if(h==IntPtr.Zero||h.ToInt64()==-1) throw new InvalidOperationException("OPEN_FAILED");
        return h;
    }
    private static TreeInfo TreeValidate(IntPtr h,bool directory) {
        TreeInfo i;
        if(!GetFileInformationByHandle(h,out i)) throw new InvalidOperationException("METADATA_FAILED");
        if((i.Attributes&0x400)!=0) throw new InvalidOperationException("REPARSE_REJECTED");
        if(((i.Attributes&0x10)!=0)!=directory) throw new InvalidOperationException("TYPE_CHANGED");
        if(!directory && i.Links!=1) throw new InvalidOperationException("HARDLINK_REJECTED");
        return i;
    }
    private static byte[] TreeDescriptor(IntPtr h) {
        IntPtr owner,group,dacl,sacl,descriptor;
        uint error=GetSecurityInfo(h,SE_FILE_OBJECT,OWNER_SECURITY_INFORMATION|2U|DACL_SECURITY_INFORMATION,out owner,out group,out dacl,out sacl,out descriptor);
        if(error!=0) throw new InvalidOperationException("ACL_READ_FAILED");
        try {
            uint length=GetSecurityDescriptorLength(descriptor);
            if(length==0||length>1048576) throw new InvalidOperationException("ACL_SIZE_REJECTED");
            byte[] bytes=new byte[length]; Marshal.Copy(descriptor,bytes,0,(int)length); return bytes;
        } finally { if(descriptor!=IntPtr.Zero) LocalFree(descriptor); }
    }
    private static System.Collections.Generic.List<TreeEntry> TreeChildren(IntPtr h,TreeEntry parent) {
        var children=new System.Collections.Generic.List<TreeEntry>();
        IntPtr buffer=Marshal.AllocHGlobal(65536); bool restart=true;
        try {
            for(;;) {
                IoStatus io=new IoStatus();
                int status=NtQueryDirectoryFile(h,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,ref io,buffer,65536,1,false,IntPtr.Zero,restart); restart=false;
                if(status==unchecked((int)0x80000006)) break;
                if(status!=0) throw new InvalidOperationException("ENUMERATION_FAILED");
                ulong used=io.Information.ToUInt64(); if(used<64||used>65536) throw new InvalidOperationException("ENUMERATION_INVALID");
                int offset=0;
                for(;;) {
                    if((ulong)offset+64>used) throw new InvalidOperationException("ENUMERATION_INVALID");
                    IntPtr at=IntPtr.Add(buffer,offset); uint next=unchecked((uint)Marshal.ReadInt32(at));
                    uint attrs=unchecked((uint)Marshal.ReadInt32(at,56)); int length=Marshal.ReadInt32(at,60);
                    if(length<0||(length&1)!=0||(ulong)offset+64+(uint)length>used) throw new InvalidOperationException("ENUMERATION_INVALID");
                    string name=Marshal.PtrToStringUni(IntPtr.Add(at,64),length/2);
                    if(name!="."&&name!="..") {
                        if(String.IsNullOrEmpty(name)||name.IndexOfAny(new char[]{'\\','/',':','\0'})>=0) throw new InvalidOperationException("NAME_REJECTED");
                        if((attrs&0x400)!=0) throw new InvalidOperationException("REPARSE_REJECTED");
                        if(children.Count>=4096) throw new InvalidOperationException("NODE_LIMIT");
                        children.Add(new TreeEntry{Path=System.IO.Path.Combine(parent.Path,name),Relative=parent.Relative+"/"+name,Depth=parent.Depth+1,Directory=(attrs&0x10)!=0});
                    }
                    if(next==0) break;
                    if(next<64||(next&7)!=0||(ulong)offset+next>=used) throw new InvalidOperationException("ENUMERATION_INVALID");
                    offset+=checked((int)next);
                }
            }
        } finally { Marshal.FreeHGlobal(buffer); }
        children.Sort(delegate(TreeEntry a,TreeEntry b){return StringComparer.Ordinal.Compare(a.Relative,b.Relative);});
        return children;
    }
    public static TreeReport TreeScan(string product,bool apply,string expectedHash) {
        var report=new TreeReport(); var pins=new System.Collections.Generic.List<IntPtr>();
        var canonical=new System.Text.StringBuilder(); long descriptorBytes=0; var repairs=new System.Collections.Generic.List<RepairItem>();
        try {
            using(WindowsIdentity identity=WindowsIdentity.GetCurrent()) {
                SecurityIdentifier current=identity.User;
                var ancestors=new System.Collections.Generic.List<string>();
                for(var d=new System.IO.DirectoryInfo(product);d!=null;d=d.Parent) ancestors.Add(d.FullName);
                ancestors.Reverse();
                foreach(string p in ancestors) { IntPtr h=TreeOpen(p,true); pins.Add(h); TreeValidate(h,true); }
                var productSd=new RawSecurityDescriptor(TreeDescriptor(pins[pins.Count-1]),0);
                if(!productSd.Owner.Equals(current)) throw new InvalidOperationException("PRODUCT_OWNER_REJECTED");
                var stack=new System.Collections.Generic.Stack<TreeEntry>();
                stack.Push(new TreeEntry{Path=System.IO.Path.Combine(product,"secrets"),Relative="secrets",Directory=true});
                stack.Push(new TreeEntry{Path=System.IO.Path.Combine(product,"data"),Relative="data",Directory=true});
                while(stack.Count!=0) {
                    if(report.NodesScanned>=4096) throw new InvalidOperationException("NODE_LIMIT");
                    TreeEntry e=stack.Pop(); if(e.Depth>16) throw new InvalidOperationException("DEPTH_LIMIT");
                    IntPtr h=TreeOpen(e.Path,e.Directory,apply); pins.Add(h); TreeInfo info=TreeValidate(h,e.Directory);
                    byte[] descriptor=TreeDescriptor(h); repairs.Add(new RepairItem{Handle=h,Original=descriptor,Relative=e.Relative,Info=info}); descriptorBytes+=descriptor.Length;
                    if(descriptorBytes>8388608)throw new InvalidOperationException("ACL_TOTAL_LIMIT");
                    var sd=new RawSecurityDescriptor(descriptor,0);
                    report.NodesScanned++; if(e.Directory)report.DirectoriesScanned++;else report.FilesScanned++;
                    if(sd.Owner.Equals(current))report.CurrentUserOwners++;
                    else if(sd.Owner.IsWellKnown(WellKnownSidType.BuiltinAdministratorsSid))report.AdminOwners++;
                    else if(sd.Owner.IsWellKnown(WellKnownSidType.LocalSystemSid))report.SystemOwners++; else report.OtherOwners++;
                    bool deny=false,unknown=false,unsupported=false,logonRead=false,logonBroad=false; uint currentAllow=0;
                    if(sd.DiscretionaryAcl==null) unsupported=true;
                    else foreach(GenericAce ace in sd.DiscretionaryAcl) {
                        var common=ace as CommonAce;
                        if(common==null||common.IsCallback) { unsupported=true;continue; }
                        if(common.AceQualifier!=AceQualifier.AccessAllowed) { deny=true;continue; }
                        if((common.AceFlags&AceFlags.InheritOnly)!=0) continue;
                        var sid=common.SecurityIdentifier; uint mask=unchecked((uint)common.AccessMask);
                        if(sid.Equals(current))currentAllow|=mask;
                        else if(sid.IsWellKnown(WellKnownSidType.LocalSystemSid)||sid.IsWellKnown(WellKnownSidType.BuiltinAdministratorsSid)){}
                        else if(System.Text.RegularExpressions.Regex.IsMatch(sid.Value,@"^S-1-5-5-[0-9]+-[0-9]+$")) { if((mask&~1179817U)==0)logonRead=true;else logonBroad=true; }
                        else unknown=true;
                    }
                    if(deny)report.DenyNodes++;if(unknown)report.UnknownTrusteeNodes++;if(unsupported)report.UnsupportedAceNodes++;
                    if(logonRead)report.LogonReadOnlyNodes++;if(logonBroad)report.LogonBroadNodes++;
                    if((currentAllow&2032127U)!=2032127U)report.MissingUserFullControlNodes++;
                    canonical.Append(e.Relative).Append('|').Append(info.Volume).Append('|').Append(info.IndexHigh).Append('|').Append(info.IndexLow).Append('|').Append(Convert.ToBase64String(descriptor)).Append('\n');
                    if(e.Directory) { var children=TreeChildren(h,e); if(stack.Count+children.Count+report.NodesScanned>4096)throw new InvalidOperationException("NODE_LIMIT"); for(int j=children.Count-1;j>=0;j--)stack.Push(children[j]); }
                }
                using(var sha=System.Security.Cryptography.SHA256.Create()) report.PlanHash=BitConverter.ToString(sha.ComputeHash(System.Text.Encoding.UTF8.GetBytes(canonical.ToString()))).Replace("-","").ToLowerInvariant();
                if(apply) {
                    if(expectedHash=="8936868301694d3f025d04bcbd3f0ffb90a597b831aebcd46c38160936620ab8") {
                        // The old snapshot did not request GROUP. Bind recovery to
                        // the requested owner/DACL/identity fields, not that absent field.
                        var audit=AuditRecovery(product);
                        if(audit.ErrorCode!="OK"||audit.Rows.Count!=9)throw new InvalidOperationException("ORIGINAL_BACKUP_AUDIT_FAILED");
                        foreach(var row in audit.Rows)if(!row.IdentityMatches||!row.OwnerMatches||!row.ProtectionMatches||!row.OriginalAceBytesMatch||row.OriginalRevision!=row.CurrentRevision)throw new InvalidOperationException("ORIGINAL_OWNER_OR_DACL_CHANGED");
                    }else if(!String.Equals(expectedHash,report.PlanHash,StringComparison.Ordinal))throw new InvalidOperationException("PLAN_CHANGED");
                    if(report.NodesScanned!=9||report.DirectoriesScanned!=2||report.FilesScanned!=7||report.AdminOwners!=9||report.DenyNodes!=0||report.UnknownTrusteeNodes!=0||report.UnsupportedAceNodes!=0||report.LogonBroadNodes!=0)throw new InvalidOperationException("EXPECTED_LEGACY_LAYOUT_REQUIRED");
                    foreach(var item in repairs) {
                        var raw=new RawSecurityDescriptor(item.Original,0);
                        if((raw.ControlFlags&ControlFlags.DiscretionaryAclProtected)!=0)throw new InvalidOperationException("EXPECTED_LEGACY_ACL_REQUIRED");
                        foreach(GenericAce ace in raw.DiscretionaryAcl)if((ace.AceFlags&(AceFlags.ContainerInherit|AceFlags.ObjectInherit|AceFlags.Inherited))!=0)throw new InvalidOperationException("INHERITING_ACL_REJECTED");
                        raw.DiscretionaryAcl.InsertAce(raw.DiscretionaryAcl.Count,new CommonAce(AceFlags.None,AceQualifier.AccessAllowed,2032127,current,false,null));
                        item.Updated=new byte[raw.BinaryLength];raw.GetBinaryForm(item.Updated,0);
                        if(!SameDaclAndOwner(item.Original,TreeDescriptor(item.Handle)))throw new InvalidOperationException("ACL_CHANGED");
                    }
                    LockExistingStoreFile(product,"data/database.lock",repairs,pins);
                    LockExistingStoreFile(product,"secrets/database-key.lock",repairs,pins);
                    report.RecoveryId=Guid.NewGuid().ToString("N");
                    SaveRecovery(product,report.RecoveryId,repairs,current,report.PlanHash,pins);
                    report.BackupSaved=true;
                    int changed=0;
                    try {
                        foreach(var item in repairs) {
                            if(!SameDaclAndOwner(item.Original,TreeDescriptor(item.Handle)))throw new InvalidOperationException("ACL_CHANGED");
                            SetDacl(item.Handle,item.Updated);changed++;
                            if(!SameDaclAndOwner(item.Updated,TreeDescriptor(item.Handle)))throw new InvalidOperationException("ACL_VERIFY_FAILED");
                        }
                        report.NodesChanged=changed;report.Applied=true;
                    } catch(Exception failure) {
                        string cause=failure is InvalidOperationException?failure.Message:"REPAIR_STEP_FAILED";
                        report.FailureCause=(cause=="ACL_CHANGED"||cause=="ACL_VERIFY_FAILED"||cause=="ACL_WRITE_FAILED")?cause:"REPAIR_STEP_FAILED";
                        report.ChangedBeforeRollback=changed;
                        report.RollbackAttempted=true;bool complete=true;
                        for(int i=changed-1;i>=0;i--)try{SetDacl(repairs[i].Handle,repairs[i].Original);if(!SameDaclAndOwner(repairs[i].Original,TreeDescriptor(repairs[i].Handle)))complete=false;}catch{complete=false;}
                        report.RollbackComplete=complete;report.NodesChanged=complete?0:changed;
                        throw new InvalidOperationException(complete?"REPAIR_FAILED_ROLLED_BACK":"RECOVERY_REQUIRED");
                    }
                }

            }
        } catch(InvalidOperationException ex) { report.ErrorCode=ex.Message; report.PlanHash=null; }
          catch { report.ErrorCode="DIAGNOSTIC_FAILED";report.PlanHash=null; }
        finally { for(int i=pins.Count-1;i>=0;i--)CloseHandle(pins[i]); }
        return report;
    }

    private sealed class RepairItem { public IntPtr Handle;public string Relative;public byte[] Original,Updated;public TreeInfo Info; }
    [StructLayout(LayoutKind.Sequential)] private struct SecurityAttributes {public uint Length;public IntPtr Descriptor;public int Inherit;}
    [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] private static extern bool ConvertStringSecurityDescriptorToSecurityDescriptorW(string text,uint revision,out IntPtr descriptor,out uint size);
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] private static extern bool CreateDirectoryW(string name,ref SecurityAttributes sa);
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,EntryPoint="CreateFileW",SetLastError=true)] private static extern IntPtr CreateNewBackup(string name,uint access,uint share,ref SecurityAttributes sa,uint disposition,uint flags,IntPtr template);
    [DllImport("advapi32.dll",SetLastError=true)] private static extern bool GetSecurityDescriptorDacl(IntPtr sd,out bool present,out IntPtr dacl,out bool defaulted);
    [DllImport("advapi32.dll",SetLastError=true)] private static extern uint SetSecurityInfo(IntPtr handle,int type,uint information,IntPtr owner,IntPtr group,IntPtr dacl,IntPtr sacl);
    private static bool SameBytes(byte[] a,byte[] b) {if(a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
    private static bool SameDaclAndOwner(byte[] a,byte[] b) {
        var x=new RawSecurityDescriptor(a,0);var y=new RawSecurityDescriptor(b,0);
        if(!x.Owner.Equals(y.Owner)||!Object.Equals(x.Group,y.Group)||((x.ControlFlags^y.ControlFlags)&ControlFlags.DiscretionaryAclProtected)!=0)return false;
        byte[] xa=new byte[x.DiscretionaryAcl.BinaryLength],ya=new byte[y.DiscretionaryAcl.BinaryLength];x.DiscretionaryAcl.GetBinaryForm(xa,0);y.DiscretionaryAcl.GetBinaryForm(ya,0);return SameBytes(xa,ya);
    }
    private static void SetDacl(IntPtr handle,byte[] descriptor) {
        IntPtr memory=Marshal.AllocHGlobal(descriptor.Length);
        try {
            Marshal.Copy(descriptor,0,memory,descriptor.Length);bool present,defaulted;IntPtr dacl;
            if(!GetSecurityDescriptorDacl(memory,out present,out dacl,out defaulted)||!present||dacl==IntPtr.Zero)throw new InvalidOperationException("DACL_INVALID");
            if(SetSecurityInfo(handle,SE_FILE_OBJECT,DACL_SECURITY_INFORMATION,IntPtr.Zero,IntPtr.Zero,dacl,IntPtr.Zero)!=0)throw new InvalidOperationException("ACL_WRITE_FAILED");
        }finally{Marshal.FreeHGlobal(memory);}
    }
    private static void VerifyBackupSecurity(IntPtr handle,SecurityIdentifier current) {
        var raw=new RawSecurityDescriptor(TreeDescriptor(handle),0);
        if(!raw.Owner.Equals(current)||(raw.ControlFlags&ControlFlags.DiscretionaryAclProtected)==0||raw.DiscretionaryAcl==null||raw.DiscretionaryAcl.Count!=3)throw new InvalidOperationException("BACKUP_SECURITY_FAILED");
        foreach(GenericAce ace in raw.DiscretionaryAcl){var common=ace as CommonAce;if(common==null||common.IsCallback||common.AceQualifier!=AceQualifier.AccessAllowed||common.AccessMask!=2032127||!(common.SecurityIdentifier.Equals(current)||common.SecurityIdentifier.IsWellKnown(WellKnownSidType.LocalSystemSid)||common.SecurityIdentifier.IsWellKnown(WellKnownSidType.BuiltinAdministratorsSid)))throw new InvalidOperationException("BACKUP_SECURITY_FAILED");}
    }
    private static void LockExistingStoreFile(string product,string relative,System.Collections.Generic.List<RepairItem> records,System.Collections.Generic.List<IntPtr> pins) {
        RepairItem expected=records.Find(delegate(RepairItem item){return item.Relative==relative;});
        if(expected==null)throw new InvalidOperationException("EXISTING_STORE_LOCK_REQUIRED");
        IntPtr handle=CreateFileW(System.IO.Path.Combine(product,relative.Replace('/',System.IO.Path.DirectorySeparatorChar)),0xC0000000U,0,IntPtr.Zero,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_BACKUP_SEMANTICS,IntPtr.Zero);
        if(handle==IntPtr.Zero||handle.ToInt64()==-1)throw new InvalidOperationException("STORE_LOCK_UNAVAILABLE");
        pins.Add(handle);TreeInfo actual=TreeValidate(handle,false);
        if(actual.Volume!=expected.Info.Volume||actual.IndexHigh!=expected.Info.IndexHigh||actual.IndexLow!=expected.Info.IndexLow)throw new InvalidOperationException("STORE_LOCK_CHANGED");
    }
    private static void SaveRecovery(string product,string id,System.Collections.Generic.List<RepairItem> records,SecurityIdentifier current,string hash,System.Collections.Generic.List<IntPtr> pins) {
        string folder=System.IO.Path.Combine(product,"storage-access-recovery-"+id);
        string sddl="O:"+current.Value+"D:P(A;;FA;;;"+current.Value+")(A;;FA;;;SY)(A;;FA;;;BA)";
        IntPtr sd;uint length;
        if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl,1,out sd,out length))throw new InvalidOperationException("BACKUP_SECURITY_FAILED");
        try {
            var sa=new SecurityAttributes{Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)),Descriptor=sd};
            if(!CreateDirectoryW(folder,ref sa))throw new InvalidOperationException("BACKUP_CREATE_FAILED");
            IntPtr dir=TreeOpen(folder,true);pins.Add(dir);TreeValidate(dir,true);VerifyBackupSecurity(dir,current);
            IntPtr file=CreateNewBackup(System.IO.Path.Combine(folder,"acl-backup.bin"),0x40000000|READ_CONTROL,0,ref sa,1,FILE_FLAG_OPEN_REPARSE_POINT,IntPtr.Zero);
            if(file==IntPtr.Zero||file.ToInt64()==-1)throw new InvalidOperationException("BACKUP_CREATE_FAILED");
            using(var safe=new Microsoft.Win32.SafeHandles.SafeFileHandle(file,true)) {
                TreeValidate(file,false);VerifyBackupSecurity(file,current);
                using(var stream=new System.IO.FileStream(safe,System.IO.FileAccess.Write)) {
                    using(var writer=new System.IO.BinaryWriter(stream,System.Text.Encoding.UTF8,true)) {
                        writer.Write("HANTONG-ACL-RECOVERY-1");writer.Write(current.Value);writer.Write(hash);writer.Write(records.Count);
                        foreach(var item in records){writer.Write(item.Relative);writer.Write(item.Info.Volume);writer.Write(item.Info.IndexHigh);writer.Write(item.Info.IndexLow);writer.Write(item.Original.Length);writer.Write(item.Original);writer.Write(item.Updated.Length);writer.Write(item.Updated);}
                        writer.Flush();stream.Flush(true);
                    }
                }
            }
        }finally{LocalFree(sd);}
    }

    public sealed class AuditRow {
        public int Index,OriginalRevision,CurrentRevision,OriginalAceCount,CurrentAceCount;
        public bool IdentityMatches,OwnerMatches,GroupMatches,ProtectionMatches,OriginalAceBytesMatch,PlannedAceBytesMatch,DescriptorBytesMatch,CurrentUserFullControl;
    }
    public sealed class AuditReport {public string ErrorCode="OK";public System.Collections.Generic.List<AuditRow> Rows=new System.Collections.Generic.List<AuditRow>();}
    private static bool AuditSameBytes(byte[] a,byte[] b){if(a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
    private static bool AuditSameAces(RawAcl a,RawAcl b){
        if(a==null||b==null)return a==b;if(a.Count!=b.Count)return false;
        for(int i=0;i<a.Count;i++){var x=new byte[a[i].BinaryLength];var y=new byte[b[i].BinaryLength];a[i].GetBinaryForm(x,0);b[i].GetBinaryForm(y,0);if(!AuditSameBytes(x,y))return false;}return true;
    }
    private static byte[] AuditReadBlob(System.IO.BinaryReader reader){int length=reader.ReadInt32();if(length<1||length>1048576)throw new InvalidOperationException("BACKUP_FORMAT_INVALID");byte[] bytes=reader.ReadBytes(length);if(bytes.Length!=length)throw new InvalidOperationException("BACKUP_FORMAT_INVALID");return bytes;}
    private static void AuditBackupSecurity(IntPtr handle){
        using(var identity=WindowsIdentity.GetCurrent()){
            var raw=new RawSecurityDescriptor(TreeDescriptor(handle),0);
            if(!raw.Owner.Equals(identity.User)||(raw.ControlFlags&ControlFlags.DiscretionaryAclProtected)==0||raw.DiscretionaryAcl==null||raw.DiscretionaryAcl.Count!=3)throw new InvalidOperationException("BACKUP_SECURITY_FAILED");
            foreach(GenericAce ace in raw.DiscretionaryAcl){var common=ace as CommonAce;if(common==null||common.IsCallback||common.AceQualifier!=AceQualifier.AccessAllowed||common.AccessMask!=2032127||!(common.SecurityIdentifier.Equals(identity.User)||common.SecurityIdentifier.IsWellKnown(WellKnownSidType.LocalSystemSid)||common.SecurityIdentifier.IsWellKnown(WellKnownSidType.BuiltinAdministratorsSid)))throw new InvalidOperationException("BACKUP_SECURITY_FAILED");}
        }
    }
    public static AuditReport AuditRecovery(string product) {
        var output=new AuditReport();var pins=new System.Collections.Generic.List<IntPtr>();
        try{
            var ancestors=new System.Collections.Generic.List<string>();for(var d=new System.IO.DirectoryInfo(product);d!=null;d=d.Parent)ancestors.Add(d.FullName);ancestors.Reverse();
            foreach(string path in ancestors){IntPtr h=TreeOpen(path,true);pins.Add(h);TreeValidate(h,true);}
            string backupDir=System.IO.Path.Combine(product,"storage-access-recovery-177ccd897d1e4e509c094e5a3c04f24c");
            IntPtr folder=TreeOpen(backupDir,true);pins.Add(folder);TreeValidate(folder,true);AuditBackupSecurity(folder);
            IntPtr file=CreateFileW(System.IO.Path.Combine(backupDir,"acl-backup.bin"),1|READ_CONTROL|FILE_READ_ATTRIBUTES,FILE_SHARE_READ,IntPtr.Zero,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,IntPtr.Zero);
            if(file==IntPtr.Zero||file.ToInt64()==-1)throw new InvalidOperationException("BACKUP_OPEN_FAILED");
            using(var safe=new Microsoft.Win32.SafeHandles.SafeFileHandle(file,true)){
                TreeInfo bi=TreeValidate(file,false);AuditBackupSecurity(file);if(bi.SizeHigh!=0||bi.SizeLow>8388608)throw new InvalidOperationException("BACKUP_SIZE_REJECTED");
                using(var stream=new System.IO.FileStream(safe,System.IO.FileAccess.Read))using(var reader=new System.IO.BinaryReader(stream,System.Text.Encoding.UTF8)){
                    if(reader.ReadString()!="HANTONG-ACL-RECOVERY-1")throw new InvalidOperationException("BACKUP_FORMAT_INVALID");
                    using(var identity=WindowsIdentity.GetCurrent()){
                        if(reader.ReadString()!=identity.User.Value)throw new InvalidOperationException("BACKUP_USER_MISMATCH");
                        if(reader.ReadString()!="8936868301694d3f025d04bcbd3f0ffb90a597b831aebcd46c38160936620ab8"||reader.ReadInt32()!=9)throw new InvalidOperationException("BACKUP_PLAN_MISMATCH");
                        for(int i=0;i<9;i++){
                            string relative=reader.ReadString();string[] parts=relative.Split('/');
                            if(parts.Length<1||parts.Length>2||(parts[0]!="data"&&parts[0]!="secrets"))throw new InvalidOperationException("BACKUP_PATH_REJECTED");
                            foreach(string part in parts)if(part.Length==0||part=="."||part==".."||part.IndexOfAny(new char[]{'\\',':','\0'})>=0)throw new InvalidOperationException("BACKUP_PATH_REJECTED");
                            uint volume=reader.ReadUInt32(),high=reader.ReadUInt32(),low=reader.ReadUInt32();
                            byte[] original=AuditReadBlob(reader),planned=AuditReadBlob(reader);
                            string nodePath=product;foreach(string part in parts)nodePath=System.IO.Path.Combine(nodePath,part);
                            IntPtr node=TreeOpen(nodePath,parts.Length==1);pins.Add(node);TreeInfo info=TreeValidate(node,parts.Length==1);
                            var currentBytes=TreeDescriptor(node);var old=new RawSecurityDescriptor(original,0);var next=new RawSecurityDescriptor(planned,0);var current=new RawSecurityDescriptor(currentBytes,0);
                            uint userMask=0;if(current.DiscretionaryAcl!=null)foreach(GenericAce ace in current.DiscretionaryAcl){var c=ace as CommonAce;if(c!=null&&c.AceQualifier==AceQualifier.AccessAllowed&&c.SecurityIdentifier.Equals(identity.User)&&(c.AceFlags&AceFlags.InheritOnly)==0)userMask|=unchecked((uint)c.AccessMask);}
                            output.Rows.Add(new AuditRow{Index=i,IdentityMatches=info.Volume==volume&&info.IndexHigh==high&&info.IndexLow==low,OwnerMatches=Object.Equals(old.Owner,current.Owner),GroupMatches=Object.Equals(old.Group,current.Group),ProtectionMatches=((old.ControlFlags^current.ControlFlags)&ControlFlags.DiscretionaryAclProtected)==0,OriginalRevision=old.DiscretionaryAcl==null?-1:old.DiscretionaryAcl.Revision,CurrentRevision=current.DiscretionaryAcl==null?-1:current.DiscretionaryAcl.Revision,OriginalAceCount=old.DiscretionaryAcl==null?-1:old.DiscretionaryAcl.Count,CurrentAceCount=current.DiscretionaryAcl==null?-1:current.DiscretionaryAcl.Count,OriginalAceBytesMatch=AuditSameAces(old.DiscretionaryAcl,current.DiscretionaryAcl),PlannedAceBytesMatch=AuditSameAces(next.DiscretionaryAcl,current.DiscretionaryAcl),DescriptorBytesMatch=AuditSameBytes(original,currentBytes),CurrentUserFullControl=(userMask&2032127U)==2032127U});
                        }
                        if(stream.Position!=stream.Length)throw new InvalidOperationException("BACKUP_FORMAT_INVALID");
                    }
                }
            }
        }catch(InvalidOperationException e){output.ErrorCode=e.Message;}catch{output.ErrorCode="AUDIT_FAILED";}
        finally{for(int i=pins.Count-1;i>=0;i--)CloseHandle(pins[i]);}return output;
    }

}
'@
}


$expected='8936868301694d3f025d04bcbd3f0ffb90a597b831aebcd46c38160936620ab8'
if($Apply -and (-not $ConfirmRepair -or $ExpectedPlanHash -cne $expected)){[pscustomobject]@{Schema='hantong-storage-repair-v4';ErrorCode='EXPLICIT_CONFIRMATION_AND_EXPECTED_HASH_REQUIRED'}|ConvertTo-Json -Compress;return}
if(@(Get-Process -Name 'mail-hantong-core' -ErrorAction SilentlyContinue).Count -ne 0){[pscustomobject]@{Schema='hantong-storage-repair-v4';ErrorCode='CORE_MUST_BE_STOPPED'}|ConvertTo-Json -Compress;return}
$localAppData=$null;$known=[HantongStorageRepairV4Native]::GetLocalAppData([ref]$localAppData)
if($known -ne 0){[pscustomobject]@{Schema='hantong-storage-repair-v4';ErrorCode='KNOWN_FOLDER_FAILED'}|ConvertTo-Json -Compress;return}
$backupToken=[IntPtr]::Zero;$restoreToken=[IntPtr]::Zero;$elevated=$false;$backupEnabled=$false;$backupKnown=$false;$restoreEnabled=$false;$restoreKnown=$false;$backupError=0;$restoreError=0;$restoreBackupError=0;$restoreRestoreError=0;$result=$null
$backupOld=[HantongStorageRepairV4Native]::EnableBackup('SeBackupPrivilege',[ref]$backupToken,[ref]$elevated,[ref]$backupEnabled,[ref]$backupKnown,[ref]$backupError)
try{
 if($Apply){$restoreOld=[HantongStorageRepairV4Native]::EnableBackup('SeRestorePrivilege',[ref]$restoreToken,[ref]$elevated,[ref]$restoreEnabled,[ref]$restoreKnown,[ref]$restoreError)}
 if($backupEnabled -and (-not $Apply -or $restoreEnabled)){$result=[HantongStorageRepairV4Native]::TreeScan((Join-Path $localAppData 'mail-hantong'),[bool]$Apply,$expected)}
}finally{
 if($restoreToken -ne [IntPtr]::Zero){if($restoreKnown){$restoreRestoreError=[HantongStorageRepairV4Native]::RestoreBackup('SeRestorePrivilege',$restoreToken,[uint32]$restoreOld)};[HantongStorageRepairV4Native]::CloseToken($restoreToken)}
 if($backupToken -ne [IntPtr]::Zero){if($backupKnown){$restoreBackupError=[HantongStorageRepairV4Native]::RestoreBackup('SeBackupPrivilege',$backupToken,[uint32]$backupOld)};[HantongStorageRepairV4Native]::CloseToken($backupToken)}
}
[pscustomobject]@{Schema='hantong-storage-repair-v4';Mode=if($Apply){'apply'}else{'dry_run'};IsElevated=$elevated;BackupPrivilegeEnableError=$backupError;RestorePrivilegeEnableError=$restoreError;BackupPrivilegeRestoreError=$restoreBackupError;RestorePrivilegeRestoreError=$restoreRestoreError;Succeeded=($null -ne $result -and $result.ErrorCode -eq 'OK' -and $restoreBackupError -eq 0 -and $restoreRestoreError -eq 0);Result=$result}|ConvertTo-Json -Depth 5 -Compress
}
