#!/usr/bin/env python3
"""Generate the checked-in Xcode project deterministically. No third-party dependency."""
from pathlib import Path
import hashlib, json
R=Path(__file__).resolve().parents[1]
objects={}
def ident(key): return hashlib.sha256(key.encode()).hexdigest()[:24].upper()
def obj(key,**values):
 i=ident(key);objects[i]=values;return i
def quote(x): return json.dumps(str(x))
def encode(x,depth=0):
 if isinstance(x,dict): return '{\n'+''.join('  '*(depth+1)+quote(k)+' = '+encode(v,depth+1)+';\n' for k,v in x.items())+'  '*depth+'}'
 if isinstance(x,list): return '( '+', '.join(encode(v,depth) for v in x)+' )'
 return quote(x)
app_files=['App/HindSightApp.swift','App/ContentView.swift','App/WorkbenchChrome.swift','App/AnalystSurface.swift','App/AnalysisNavigation.swift','App/AnalysisEvidenceCore.swift','App/AnalysisContinuity.swift','App/DisassemblyModel.swift','App/GraphModel.swift','App/GraphWorkspace.swift','App/BinaryWorkspaceModel.swift','App/BinaryWorkspace.swift','App/BinaryDiffModel.swift','App/BinaryDiffWorkspace.swift','App/RuntimeCorrelationModel.swift','App/RuntimeCorrelationWorkspace.swift','App/TraceExplorerModel.swift','App/TraceExplorerWorkspace.swift','App/EvidenceGraphModel.swift','App/EvidenceGraphWorkspace.swift','App/InvestigationLedgerModel.swift','App/InvestigationLedgerWorkspace.swift','App/WorkspaceCatalog.swift','App/WorkbenchModel.swift','App/CoordinatorClient.swift','App/TargetWorkspace.swift','Shared/CoordinatorProtocol.swift','Shared/WorkbenchMessages.swift','Shared/DebugMessages.swift','Shared/ScriptingMessages.swift','Shared/VirtualizationMessages.swift','Shared/FirmwareRuntimeMessages.swift','Shared/TargetModel.swift','Shared/HostPlatform.swift']
svc_files=['PlatformServices/CoordinatorService/VerifiedPageCache.swift','PlatformServices/CoordinatorService/main.swift','PlatformServices/CoordinatorService/ProjectStore.swift','PlatformServices/CoordinatorService/WorkerSupervisor.swift','Shared/CoordinatorProtocol.swift','Shared/WorkbenchMessages.swift','Shared/DebugMessages.swift','Shared/ScriptingMessages.swift','Shared/VirtualizationMessages.swift','PlatformServices/CoordinatorService/DebugServer.swift','PlatformServices/CoordinatorService/ScriptingHost.swift','PlatformServices/CoordinatorService/VirtualizationHost.swift','PlatformServices/CoordinatorService/FirmwareRuntimeHost.swift','Shared/FirmwareRuntimeMessages.swift','Shared/HostPlatform.swift','native/debug/debug_server.c','native/macho/decode.c','native/virtualization/hv_vm.c']
all_files=sorted(set(app_files+svc_files+['Bridge/HindSightBridge.h','native/debug/debug_server.h','native/macho/decode.h','native/virtualization/hv_vm.h','Config/App-Info.plist','Config/Coordinator-Info.plist','Config/Coordinator.entitlements','Resources/schema.sql','README.md']))
refs={p:obj('file:'+p,isa='PBXFileReference',lastKnownFileType=('sourcecode.swift' if p.endswith('.swift') else 'sourcecode.c.h' if p.endswith('.h') else 'sourcecode.c.c' if p.endswith('.c') else 'text.plist.xml' if p.endswith('.plist') else 'text'),path=p,sourceTree='<group>') for p in all_files}
app_product=obj('app-product',isa='PBXFileReference',explicitFileType='wrapper.application',includeInIndex=0,path='HindSight.app',sourceTree='BUILT_PRODUCTS_DIR')
svc_product=obj('svc-product',isa='PBXFileReference',explicitFileType='wrapper.xpc-service',includeInIndex=0,path='CoordinatorService.xpc',sourceTree='BUILT_PRODUCTS_DIR')
products=obj('products',isa='PBXGroup',children=[app_product,svc_product],name='Products',sourceTree='<group>')
main=obj('main-group',isa='PBXGroup',children=list(refs.values())+[products],sourceTree='<group>')
def sources(name,files):
 builds=[obj('build:'+name+':'+p,isa='PBXBuildFile',fileRef=refs[p]) for p in files]
 return obj(name+'-sources',isa='PBXSourcesBuildPhase',buildActionMask=2147483647,files=builds,runOnlyForDeploymentPostprocessing=0)
app_sources=sources('app',app_files);svc_sources=sources('svc',svc_files)
schema_build=obj('schema-build',isa='PBXBuildFile',fileRef=refs['Resources/schema.sql'])
resources=obj('svc-resources',isa='PBXResourcesBuildPhase',buildActionMask=2147483647,files=[schema_build],runOnlyForDeploymentPostprocessing=0)
worker_phase=obj('worker-phase',isa='PBXShellScriptBuildPhase',buildActionMask=2147483647,files=[],inputPaths=[],outputPaths=['$(TARGET_BUILD_DIR)/$(EXECUTABLE_FOLDER_PATH)/reworker'],name='Embed Rust worker',runOnlyForDeploymentPostprocessing=0,shellPath='/bin/bash',shellScript='"${SRCROOT}/scripts/embed-worker.sh"\n',alwaysOutOfDate=1)
vector_emu_phase=obj('vector-emu-phase',isa='PBXShellScriptBuildPhase',buildActionMask=2147483647,files=[],inputPaths=[],outputPaths=['$(TARGET_BUILD_DIR)/$(EXECUTABLE_FOLDER_PATH)/vg-emu-server'],name='Build VectorEmu',runOnlyForDeploymentPostprocessing=0,shellPath='/bin/bash',shellScript='"${SRCROOT}/scripts/build-vector-emu.sh"\n',alwaysOutOfDate=1)
rust_phase=obj('rust-phase',isa='PBXShellScriptBuildPhase',buildActionMask=2147483647,files=[],inputPaths=[],outputPaths=['$(CONFIGURATION_TEMP_DIR)/HindSightRustArtifacts/libreworkbench_bridge.a','$(CONFIGURATION_TEMP_DIR)/HindSightRustArtifacts/reworker'],name='Build Rust bridge',runOnlyForDeploymentPostprocessing=0,shellPath='/bin/bash',shellScript='"${SRCROOT}/scripts/build-rust.sh"\n',alwaysOutOfDate=1)
embed_file=obj('embed-svc',isa='PBXBuildFile',fileRef=svc_product,settings={'ATTRIBUTES':['CodeSignOnCopy','RemoveHeadersOnCopy']})
embed=obj('embed-phase',isa='PBXCopyFilesBuildPhase',buildActionMask=2147483647,dstPath='Contents/XPCServices',dstSubfolderSpec=1,files=[embed_file],name='Embed XPC Service',runOnlyForDeploymentPostprocessing=0)
def configs(name,settings):
 ids=[]
 for config in ['Debug','Release']:
  bs=dict(settings)
  if name=='app' and config=='Debug':
   # Swift 6.4 can abort during SILGen on this SwiftUI target when view
   # declarations are emitted together as a batch.
   bs['SWIFT_ENABLE_BATCH_MODE']='NO'
  if name=='project':
   bs.update({'SWIFT_OPTIMIZATION_LEVEL':'-Onone' if config=='Debug' else '-O','DEBUG_INFORMATION_FORMAT':'dwarf' if config=='Debug' else 'dwarf-with-dsym'})
  ids.append(obj(name+'-config-'+config,isa='XCBuildConfiguration',buildSettings=bs,name=config))
 return obj(name+'-config-list',isa='XCConfigurationList',buildConfigurations=ids,defaultConfigurationIsVisible=0,defaultConfigurationName='Release')
project_conf=configs('project',{'ARCHS':'arm64 x86_64','SDKROOT':'macosx','MACOSX_DEPLOYMENT_TARGET':'14.0','SWIFT_VERSION':'5.0','CLANG_ENABLE_MODULES':'YES','CODE_SIGN_STYLE':'Manual','CODE_SIGN_IDENTITY':'-','ENABLE_HARDENED_RUNTIME':'YES','GCC_C_LANGUAGE_STANDARD':'c17','SWIFT_STRICT_CONCURRENCY':'targeted'})
app_conf=configs('app',{'PRODUCT_NAME':'HindSight','PRODUCT_BUNDLE_IDENTIFIER':'ca.zensoftware.VectorGlass','INFOPLIST_FILE':'Config/App-Info.plist','GENERATE_INFOPLIST_FILE':'NO','LD_RUNPATH_SEARCH_PATHS':['$(inherited)','@executable_path/../Frameworks'],'ENABLE_APP_SANDBOX':'NO'})
svc_conf=configs('svc',{'PRODUCT_NAME':'CoordinatorService','PRODUCT_BUNDLE_IDENTIFIER':'ca.zensoftware.VectorGlass.CoordinatorService','INFOPLIST_FILE':'Config/Coordinator-Info.plist','GENERATE_INFOPLIST_FILE':'NO','CODE_SIGN_ENTITLEMENTS':'Config/Coordinator.entitlements','SWIFT_OBJC_BRIDGING_HEADER':'Bridge/HindSightBridge.h','LIBRARY_SEARCH_PATHS':['$(inherited)','$(CONFIGURATION_TEMP_DIR)/HindSightRustArtifacts'],'OTHER_LDFLAGS':['$(inherited)','-lreworkbench_bridge','-liconv','-lsqlite3','-framework','Security','-framework','Hypervisor','-framework','Network'],'LD_RUNPATH_SEARCH_PATHS':['$(inherited)','@executable_path/../Frameworks','@executable_path/../../../../Frameworks'],'SKIP_INSTALL':'YES','ENABLE_APP_SANDBOX':'NO','ENABLE_USER_SCRIPT_SANDBOXING':'NO'})
svc=obj('svc-target',isa='PBXNativeTarget',buildConfigurationList=svc_conf,buildPhases=[rust_phase,svc_sources,resources,worker_phase,vector_emu_phase],buildRules=[],dependencies=[],name='CoordinatorService',productName='CoordinatorService',productReference=svc_product,productType='com.apple.product-type.xpc-service')
proxy=obj('svc-proxy',isa='PBXContainerItemProxy',containerPortal=ident('project'),proxyType=1,remoteGlobalIDString=svc,remoteInfo='CoordinatorService')
dep=obj('svc-dependency',isa='PBXTargetDependency',target=svc,targetProxy=proxy)
app=obj('app-target',isa='PBXNativeTarget',buildConfigurationList=app_conf,buildPhases=[app_sources,embed],buildRules=[],dependencies=[dep],name='HindSight',productName='HindSight',productReference=app_product,productType='com.apple.product-type.application')
project=obj('project',isa='PBXProject',attributes={'BuildIndependentTargetsInParallel':'YES','LastUpgradeCheck':'2710'},buildConfigurationList=project_conf,compatibilityVersion='Xcode 14.0',developmentRegion='en',hasScannedForEncodings=0,knownRegions=['en','Base'],mainGroup=main,productRefGroup=products,projectDirPath='',projectRoot='',targets=[app,svc])
root={'archiveVersion':1,'classes':{},'objectVersion':56,'objects':objects,'rootObject':project}
p=R/'HindSight.xcodeproj/project.pbxproj';p.parent.mkdir(exist_ok=True);p.write_text('// !$*UTF8*$!\n'+encode(root)+'\n')
workspace=R/'HindSight.xcworkspace/contents.xcworkspacedata';workspace.parent.mkdir(exist_ok=True);workspace.write_text('<?xml version="1.0" encoding="UTF-8"?>\n<Workspace version="1.0">\n  <FileRef location="group:HindSight.xcodeproj"></FileRef>\n</Workspace>\n')
scheme=R/'HindSight.xcodeproj/xcshareddata/xcschemes/HindSight.xcscheme';scheme.parent.mkdir(parents=True,exist_ok=True)
ref=f'<BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="{app}" BuildableName="HindSight.app" BlueprintName="HindSight" ReferencedContainer="container:HindSight.xcodeproj"/>'
scheme.write_text(f'''<?xml version="1.0" encoding="UTF-8"?>
<Scheme LastUpgradeVersion="2710" version="1.3">
 <BuildAction parallelizeBuildables="YES" buildImplicitDependencies="YES"><BuildActionEntries><BuildActionEntry buildForTesting="YES" buildForRunning="YES" buildForProfiling="YES" buildForArchiving="YES" buildForAnalyzing="YES">{ref}</BuildActionEntry></BuildActionEntries></BuildAction>
 <TestAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" shouldUseLaunchSchemeArgsEnv="YES"><Testables/></TestAction>
 <LaunchAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" launchStyle="0" useCustomWorkingDirectory="NO" ignoresPersistentStateOnLaunch="NO" debugDocumentVersioning="YES" debugServiceExtension="internal" allowLocationSimulation="NO"><BuildableProductRunnable runnableDebuggingMode="0">{ref}</BuildableProductRunnable></LaunchAction>
 <ProfileAction buildConfiguration="Release" shouldUseLaunchSchemeArgsEnv="YES" savedToolIdentifier="" useCustomWorkingDirectory="NO" debugDocumentVersioning="YES"><BuildableProductRunnable runnableDebuggingMode="0">{ref}</BuildableProductRunnable></ProfileAction>
 <AnalyzeAction buildConfiguration="Debug"/>
 <ArchiveAction buildConfiguration="Release" revealArchiveInOrganizer="YES"/>
</Scheme>
''')
# Machine-readable topology for the source checker; generated alongside pbxproj.
(R/'Config/project-topology.json').write_text(json.dumps({'files':all_files,'targets':{'HindSight':app_files,'CoordinatorService':svc_files},'embedded_service':'ca.zensoftware.VectorGlass.CoordinatorService','project_sha256':hashlib.sha256(p.read_bytes()).hexdigest()},indent=2)+'\n')
print('Generated deterministic Xcode project, workspace and scheme')
