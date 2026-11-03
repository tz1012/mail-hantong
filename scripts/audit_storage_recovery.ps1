<# Read-only bounded storage metadata inventory. No repair or file-content access. #>
& {
$ErrorActionPreference='Stop'
if(-not ('HantongStorageRecoveryAuditNative' -as [type])){Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;

public static class HantongStorageRecoveryAuditNative
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
    [StructLayout(LayoutKind.Sequential)] private struct TreeInfo {
        public uint Attributes; public System.Runtime.InteropServices.ComTypes.FILETIME Creation,Access,Write;
        public uint Volume,SizeHigh,SizeLow,Links,IndexHigh,IndexLow;
    }
    [StructLayout(LayoutKind.Sequential)] private struct IoStatus { public IntPtr Status; public UIntPtr Information; }
    [DllImport("kernel32.dll",SetLastError=true)] private static extern bool GetFileInformationByHandle(IntPtr h,out TreeInfo info);
    [DllImport("ntdll.dll")] private static extern int NtQueryDirectoryFile(IntPtr h,IntPtr e,IntPtr apc,IntPtr context,ref IoStatus status,IntPtr buffer,uint size,int klass,[MarshalAs(UnmanagedType.U1)] bool single,IntPtr name,[MarshalAs(UnmanagedType.U1)] bool restart);
    public sealed class TreeReport {
        public string ErrorCode="OK", PlanHash;
        public int NodesScanned,DirectoriesScanned,FilesScanned,CurrentUserOwners,AdminOwners,SystemOwners,OtherOwners,DenyNodes,UnknownTrusteeNodes,UnsupportedAceNodes,MissingUserFullControlNodes,LogonReadOnlyNodes,LogonBroadNodes;
    }
    private sealed class TreeEntry { public string Path,Relative; public int Depth; public bool Directory; }
    private static IntPtr TreeOpen(string path,bool directory) {
        uint access=READ_CONTROL|FILE_READ_ATTRIBUTES|0x00100000U|(directory?1U:0U);
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
        uint error=GetSecurityInfo(h,SE_FILE_OBJECT,OWNER_SECURITY_INFORMATION|DACL_SECURITY_INFORMATION,out owner,out group,out dacl,out sacl,out descriptor);
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
    public static TreeReport TreeScan(string product) {
        var report=new TreeReport(); var pins=new System.Collections.Generic.List<IntPtr>();
        var canonical=new System.Text.StringBuilder(); long descriptorBytes=0;
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
                    IntPtr h=TreeOpen(e.Path,e.Directory); pins.Add(h); TreeInfo info=TreeValidate(h,e.Directory);
                    byte[] descriptor=TreeDescriptor(h); descriptorBytes+=descriptor.Length;
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
            }
        } catch(InvalidOperationException ex) { report.ErrorCode=ex.Message; report.PlanHash=null; }
          catch { report.ErrorCode="DIAGNOSTIC_FAILED";report.PlanHash=null; }
        finally { for(int i=pins.Count-1;i>=0;i--)CloseHandle(pins[i]); }
        return report;
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

$localAppData=$null
$knownFolderError=[HantongStorageRecoveryAuditNative]::GetLocalAppData([ref]$localAppData)
if($knownFolderError -ne 0){[pscustomobject]@{Schema='hantong-storage-recovery-audit-v1';ErrorCode='KNOWN_FOLDER_FAILED'}|ConvertTo-Json -Compress;return}
$token=[IntPtr]::Zero;$elevated=$false;$enabled=$false;$stateKnown=$false;$enableError=0;$restoreError=0;$result=$null
$old=[HantongStorageRecoveryAuditNative]::EnableBackup([ref]$token,[ref]$elevated,[ref]$enabled,[ref]$stateKnown,[ref]$enableError)
try {
 if($enabled){$result=[HantongStorageRecoveryAuditNative]::AuditRecovery((Join-Path $localAppData 'mail-hantong'))}
}finally{
 if($token -ne [IntPtr]::Zero){if($stateKnown){$restoreError=[HantongStorageRecoveryAuditNative]::RestoreBackup($token,[uint32]$old)};[HantongStorageRecoveryAuditNative]::CloseToken($token)}
}
if($restoreError -ne 0){$result=$null}
[pscustomobject]@{Schema='hantong-storage-recovery-audit-v1';IsElevated=$elevated;BackupPrivilegeEnableError=$enableError;BackupPrivilegeEnabled=$enabled;BackupPrivilegeRestoreError=$restoreError;SnapshotIsAtomic=$false;DiagnosticSucceeded=($enabled -and $restoreError -eq 0 -and $null -ne $result -and $result.ErrorCode -eq 'OK');Result=$result}|ConvertTo-Json -Depth 5 -Compress
}
