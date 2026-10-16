import { Composition } from 'remotion';
import { PerchIntro, durationInFrames, FPS } from './Composition';

export const RemotionRoot: React.FC = () => <Composition
  id="PerchIntro"
  component={PerchIntro}
  durationInFrames={durationInFrames}
  calculateMetadata={() => ({ durationInFrames })}
  fps={FPS}
  width={1920}
  height={1080}
/>;
