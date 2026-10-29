import {useState} from 'react';
import {createRoot} from 'react-dom/client';
import {QueryClient,QueryClientProvider} from '@tanstack/react-query';
import {HashRouter,useLocation} from 'react-router-dom';
import Work from '../src/features/Work';
import Bugs from '../src/features/Bugs';
import {emptySnapshot} from '../src/lib/api';
import type {Snapshot} from '../src/lib/types';
import AssignmentNotifications from '../src/components/AssignmentNotifications';
import '../src/styles.css';
import '../src/glass.css';
import '../src/theme.css';
function Fixture(){
 const [workspace,setWorkspace]=useState('workspace-a'); const [message,notify]=useState(''); const [loaded,setLoaded]=useState(false);const [revision,setRevision]=useState(0);const location=useLocation();
 (window as any).__loadDetails=()=>setLoaded(true);(window as any).__refreshDetails=()=>setRevision(value=>value+1);
 const detail=new URLSearchParams(window.location.search).get('detail');
 const task={id:'t1',title:'新增任务',notes:'',source:'zentao',status:'todo',sortOrder:1024,priority:'normal',revision:1};
 const bug={id:'b1',remoteId:'1',connectionId:'c1',title:'新增 BUG',status:'active',steps:'',revision:1,severity:2,priority:1,assignedTo:'tester'};
 const data={...emptySnapshot,tasks:loaded?[{...task}]:[],bugs:loaded?[{...bug}]:[]} as Snapshot;
 void revision;
 return <><header className="topbar"><span>我的空间 / 指派提醒验收</span><button onClick={()=>setWorkspace('workspace-b')}>切换空间</button><AssignmentNotifications workspaceId={workspace} notify={notify}/></header><output>{location.pathname+location.search}</output><p role="alert">{message}</p>{detail==='task'?<Work workspaceId={workspace} data={data} refresh={async()=>{}} notify={notify}/>:detail==='bug'?<Bugs workspaceId={workspace} data={data} refresh={async()=>{}} notify={notify}/>:null}</>;
}
createRoot(document.getElementById('root')!).render(<QueryClientProvider client={new QueryClient({defaultOptions:{queries:{retry:false}}})}><HashRouter><Fixture/></HashRouter></QueryClientProvider>);
