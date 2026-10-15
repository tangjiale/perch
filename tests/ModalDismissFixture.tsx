import React, {useState} from 'react';
import {createRoot} from 'react-dom/client';
import {TaskEditor} from '../src/features/Work';
import Modal from '../src/components/Modal';
import {emptySnapshot} from '../src/lib/api';
import '../src/styles.css';
import '../src/glass.css';
import '../src/theme.css';
function Fixture(){
  const [open,setOpen]=useState(true);
  const busy=new URLSearchParams(location.search).has('busy');
  return open ? busy ? <Modal title="保存中测试" busy onClose={()=>setOpen(false)}><input defaultValue="保留草稿"/></Modal> : <TaskEditor task={{id:'dismiss-task',title:'',notes:'',source:'local',status:'todo',priority:'normal',sortOrder:0}} data={emptySnapshot} refresh={async()=>{}} notify={()=>{}} onClose={()=>setOpen(false)}/> : <p>已显式关闭</p>;
}
createRoot(document.getElementById('root')!).render(<Fixture/>);
