import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Skills } from '../src/features/Resources';
import type { Skill } from '../src/lib/types';
import { emptySnapshot } from '../src/lib/api';
import '../src/styles.css';
import '../src/glass.css';
import '../src/theme.css';
import '../src/features/model-actions.css';
function Fixture() {
  const [skills, setSkills] = useState<Skill[]>([]);
  return <div style={{padding:32}}><Skills data={{...emptySnapshot,skills}} refresh={async()=>setSkills([...(window as any).skillRows])} notify={()=>{}} /></div>;
}
createRoot(document.getElementById('root')!).render(<Fixture/>);
