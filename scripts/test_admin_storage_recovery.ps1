# This harness creates and accesses only a fresh RUNNER_TEMP fixture. It never
# calls the recovery command's LocalAppData entry point and uses no credentials.
$ErrorActionPreference='Stop'
if(-not $env:RUNNER_TEMP -or -not $env:GITHUB_ACTIONS){throw 'Ephemeral CI runner required'}
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'repair_storage_access_v3.ps1')).Replace("`r`n","`n")
$native=$source.Split(@("Add-Type -TypeDefinition @'`n"),[StringSplitOptions]::None)[1].Split(@("`n'@"),[StringSplitOptions]::None)[0]
$native=$native.Replace('HantongStorageRepairV3Native','HantongStorageAdminFixtureNative')
$native=$native.Replace('public string ErrorCode="OK", PlanHash;','public string ErrorCode="OK", PlanHash,FailureStage; public int ChangedBeforeRollback; public bool OwnerEqual,GroupEqual,ProtectionEqual,AclEqual;')
$native=$native.Replace('if(!SameDaclAndOwner(item.Updated,TreeDescriptor(item.Handle)))throw new InvalidOperationException("ACL_VERIFY_FAILED");','if(!SameDaclAndOwner(item.Updated,TreeDescriptor(item.Handle))){DebugComparison(item.Updated,TreeDescriptor(item.Handle),report);throw new InvalidOperationException("ACL_VERIFY_FAILED");}')
$native=$native.Replace('if(!SameBytes(item.Original,TreeDescriptor(item.Handle)))throw new InvalidOperationException("ACL_CHANGED");','if(!SameBytes(item.Original,TreeDescriptor(item.Handle))){DebugComparison(item.Original,TreeDescriptor(item.Handle),report);throw new InvalidOperationException("ACL_CHANGED");}')
$native=$native.Replace('} catch {', '} catch(Exception failure) {')
$native=$native.Replace('report.RollbackAttempted=true;bool complete=true;','report.FailureStage=failure is InvalidOperationException?failure.Message:"NATIVE_EXCEPTION";report.ChangedBeforeRollback=changed;report.RollbackAttempted=true;bool complete=true;')
$extra=@'
    private static void DebugComparison(byte[] a,byte[] b,TreeReport report){
        var x=new RawSecurityDescriptor(a,0);var y=new RawSecurityDescriptor(b,0);
        report.OwnerEqual=Object.Equals(x.Owner,y.Owner);report.GroupEqual=Object.Equals(x.Group,y.Group);
        report.ProtectionEqual=((x.ControlFlags^y.ControlFlags)&ControlFlags.DiscretionaryAclProtected)==0;
        report.AclEqual=AuditSameAces(x.DiscretionaryAcl,y.DiscretionaryAcl);
    }
    public static void CreateFixture(string product,string primaryGroup){
        using(var identity=WindowsIdentity.GetCurrent()){
            string user=identity.User.Value;
            string productAcl="O:"+user+"D:P(A;;FA;;;"+user+")(A;;FA;;;SY)(A;;FA;;;BA)";
            string logon="S-1-5-5-991-992";
            foreach(var sid in identity.Groups)if(sid.Value.StartsWith("S-1-5-5-")){logon=sid.Value;break;}
            string group=primaryGroup=="absent"?"":primaryGroup=="admin"?"G:BA":"G:"+user;
            string legacy="O:BA"+group+"D:(A;;FA;;;SY)(A;;GRGX;;;"+logon+")(A;;FA;;;BA)";
            FixtureDirectory(product,productAcl);
            FixtureDirectory(System.IO.Path.Combine(product,"data"),legacy);
            FixtureDirectory(System.IO.Path.Combine(product,"secrets"),legacy);
            foreach(string relative in new string[]{"data/database.lock","data/mail-hantong.db","secrets/database-key.lock","secrets/database-v1.secret","secrets/one.secret","secrets/two.secret","secrets/three.secret"}){
                IntPtr sd;uint size;
                if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(legacy,1,out sd,out size))throw new InvalidOperationException("FIXTURE_SD_FAILED");
                try{
                    var sa=new SecurityAttributes{Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)),Descriptor=sd};
                    IntPtr file=CreateNewBackup(System.IO.Path.Combine(product,relative),0x40000000,0,ref sa,1,FILE_FLAG_OPEN_REPARSE_POINT,IntPtr.Zero);
                    if(file==IntPtr.Zero||file.ToInt64()==-1)throw new InvalidOperationException("FIXTURE_FILE_FAILED");
                    using(var safe=new Microsoft.Win32.SafeHandles.SafeFileHandle(file,true))using(var stream=new System.IO.FileStream(safe,System.IO.FileAccess.Write)){byte[] bytes=System.Text.Encoding.UTF8.GetBytes("synthetic-not-mail-not-secret");stream.Write(bytes,0,bytes.Length);stream.Flush(true);}
                }finally{LocalFree(sd);}
            }
        }
    }
    private static void FixtureDirectory(string path,string text){
        IntPtr sd;uint size;
        if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(text,1,out sd,out size))throw new InvalidOperationException("FIXTURE_SD_FAILED");
        try{var sa=new SecurityAttributes{Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)),Descriptor=sd};if(!CreateDirectoryW(path,ref sa))throw new InvalidOperationException("FIXTURE_DIR_FAILED");}finally{LocalFree(sd);}
    }
'@
$native=$native.Substring(0,$native.LastIndexOf('}'))+"`n"+$extra+"`n}"
Add-Type -TypeDefinition $native
if($null -eq [HantongStorageAdminFixtureNative].GetMethod('CreateFixture')){throw 'Fixture native helper missing'}
$backupToken=[IntPtr]::Zero;$restoreToken=[IntPtr]::Zero;$elevated=$false;$enabled=$false;$known=$false;$errorCode=0
$restoreEnabled=$false;$restoreKnown=$false;$restoreError=0
$old=[HantongStorageAdminFixtureNative]::EnableBackup('SeBackupPrivilege',[ref]$backupToken,[ref]$elevated,[ref]$enabled,[ref]$known,[ref]$errorCode)
$oldRestore=[HantongStorageAdminFixtureNative]::EnableBackup('SeRestorePrivilege',[ref]$restoreToken,[ref]$elevated,[ref]$restoreEnabled,[ref]$restoreKnown,[ref]$restoreError)
$failed=$false
try{
 if(-not $elevated -or -not $enabled -or -not $restoreEnabled){throw 'Real elevated backup/restore token required'}
 foreach($group in 'absent','admin','current'){
  $owned=Join-Path $env:RUNNER_TEMP ('hantong-admin-fixture-'+[guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $owned | Out-Null
  $product=Join-Path $owned 'product'
  [HantongStorageAdminFixtureNative]::CreateFixture($product,$group)
  $plan=[HantongStorageAdminFixtureNative]::TreeScan($product,$false,$null)
  if($plan.ErrorCode -ne 'OK' -or $plan.AdminOwners -ne 9 -or $plan.MissingUserFullControlNodes -ne 9 -or $plan.LogonReadOnlyNodes -ne 9){$plan|ConvertTo-Json -Compress;throw 'Fixture does not reproduce diagnosed metadata'}
  $before=@{};foreach($f in Get-ChildItem -LiteralPath (Join-Path $product 'data'),(Join-Path $product 'secrets') -File){$before[$f.FullName]=(Get-FileHash -LiteralPath $f.FullName).Hash}
  $result=[HantongStorageAdminFixtureNative]::TreeScan($product,$true,$plan.PlanHash)
  [pscustomobject]@{case=$group;elevated=$elevated;backup_enabled=$enabled;restore_enabled=$restoreEnabled;result=$result}|ConvertTo-Json -Depth 6 -Compress
  foreach($path in $before.Keys){if((Get-FileHash -LiteralPath $path).Hash -ne $before[$path]){throw 'Synthetic contents changed'}}
  if(-not $result.Applied -or $result.NodesChanged -ne 9){$failed=$true}
 }
}finally{
 if($restoreKnown){[void][HantongStorageAdminFixtureNative]::RestoreBackup('SeRestorePrivilege',$restoreToken,[uint32]$oldRestore)}
 if($known){[void][HantongStorageAdminFixtureNative]::RestoreBackup('SeBackupPrivilege',$backupToken,[uint32]$old)}
 [HantongStorageAdminFixtureNative]::CloseToken($restoreToken);[HantongStorageAdminFixtureNative]::CloseToken($backupToken)
}
if($failed){throw 'Administrator-owned recovery regression reproduced'}
